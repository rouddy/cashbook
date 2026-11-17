# Kubernetes 배포

Rocket 서버 하나가 API(`/api`)와 웹 화면(`/`)을 함께 서빙합니다. 웹 빌드 결과는 이미지에 넣지 않고 **PVC(`cashbook-web`)** 에 올려 두고,
DB는 Secret의 `DATABASE_URL`로 고릅니다 — **SQLite**(PVC `cashbook-data`의 파일) 또는 **MySQL**(7장).

```
https://cashbook.ster.pe.kr
Ingress (Traefik, TLS: cert-manager + Let's Encrypt → Secret cashbook-tls, HTTP → HTTPS 301)
  └─ Service cashbook :80
       └─ Deployment cashbook (replicas 1, Recreate)   이미지: rouddy/cashbook (Docker Hub)
            ├─ /data  ← PVC cashbook-data   cashbook.db(SQLite일 때), secret.key, backups/
            ├─ /web   ← PVC cashbook-web    releases/<릴리스>/, current → releases/<릴리스>  (읽기 전용)
            └─ /tmp   ← emptyDir
```

| 경로 | 내용 |
|---|---|
| `setup/namespace.yaml`, `setup/storage.yaml` | 네임스페이스, PVC 두 개. **한 번만 적용**하고 앱과 따로 관리 (앱을 지워도 데이터 유지) |
| `setup/cluster-issuer.yaml` | cert-manager ClusterIssuer (Let's Encrypt staging/prod, HTTP-01) |
| `app/` (kustomization) | ConfigMap, Deployment, Service, Ingress — `kubectl apply -k k8s/app` |
| `app/traefik/` (component) | Traefik용 HTTP → HTTPS 리다이렉트 Middleware (ingress-nginx면 뺌) |
| `docker/server.Dockerfile` | 서버 이미지 (Rust 빌드 → debian slim, uid 10001, sqlite3 포함) |
| `docker/web.Dockerfile` | (선택) 웹 빌드만 담은 이미지 — CI에서 Job으로 PVC에 올릴 때 |
| `docker/build_and_push.sh` | 서버 이미지 빌드 → Docker Hub `rouddy/cashbook` 푸시 → `app/kustomization.yaml` 태그 변경 |
| `docker/image_tag.sh` | 이미지 태그 규칙 (`build_and_push.sh`와 CircleCI `.circleci/config.yml`이 같이 source) |
| `tools/pvc-shell.yaml` | 두 PVC를 마운트하는 작업용 Pod (웹 업로드·DB 복원에 스크립트가 사용) |
| `tools/web-sync-job.yaml` | (선택) 웹 이미지 → PVC 복사 Job |
| `scripts/deploy-web.sh` | 웹 빌드 → PVC 새 릴리스 → `current` 교체 |
| `scripts/backup-db.sh`, `scripts/restore-db.sh` | DB 한 시점 사본 받기(SQLite·MySQL → SQLite 파일) / SQLite DB 파일로 바꾸기(SQLite 전용) |

스크립트는 현재 kubectl 컨텍스트를 씁니다. 다른 클러스터·네임스페이스는 `KUBECTL="kubectl --context 이름"`, `NAMESPACE=...`로 지정합니다.

## 준비물

- Kubernetes 1.25+ 와 `kubectl` (kustomize 내장)
- **ReadWriteOnce 볼륨을 주는 StorageClass** (k3s `local-path`, 클라우드 블록 스토리지 등).
  SQLite는 파일 잠금에 의존하므로 **NFS·SMB 같은 네트워크 파일시스템은 쓰지 마세요** (DB 손상 위험).
- Docker Hub 저장소 `rouddy/cashbook` 에 푸시할 수 있는 `docker login` (비밀번호 대신 액세스 토큰 권장)과 `docker buildx`
- Ingress 컨트롤러(k3s 기본 Traefik 기준, ingress-nginx도 가능)와 cert-manager — 4장.
  서버는 HTTPS Ingress 뒤에서 `CASHBOOK_SECURE_COOKIE=1`로 동작하도록 설정되어 있습니다.

## 1. 서버 이미지 빌드·푸시 (Docker Hub `rouddy/cashbook`)

저장소 루트에서 빌드합니다. 빌드 컨텍스트에는 허용 목록(`*.Dockerfile.dockerignore`)에 있는 서버 소스만 들어가므로
`server/data`의 DB나 `import/`의 금융 데이터가 이미지에 들어가지 않습니다.

```bash
docker login -u rouddy                    # 처음 한 번 (Docker Hub 액세스 토큰, Read & Write)

k8s/docker/build_and_push.sh
PLATFORMS=linux/arm64 k8s/docker/build_and_push.sh   # 노드 아키텍처만 빌드 (빠름)
```

태그는 `server/Cargo.toml`의 `[package] version`과 현재 브랜치로 정합니다.

| 브랜치 | 태그 | 예 |
|---|---|---|
| `master` | `<version>` | `rouddy/cashbook:0.1.0` |
| 그 밖 (detached HEAD 포함) | `<version>-<커밋 short sha>` | `rouddy/cashbook:0.1.0-5ed0dae` |

스크립트는 `docker buildx build --platform linux/amd64,linux/arm64 ... --push`로 두 아키텍처 이미지를 올리고,
`k8s/app/kustomization.yaml`의 `images`를 그 태그로 바꿉니다(클러스터에는 적용하지 않음 — 5장·8장).

```yaml
images:
  - name: cashbook-server
    newName: rouddy/cashbook
    newTag: "0.1.0-5ed0dae"
```

- 빌드 PC(Apple Silicon = arm64)와 다른 아키텍처는 QEMU 에뮬레이션으로 컴파일해서 오래 걸립니다. 노드 아키텍처를 알면
  `PLATFORMS`로 하나만 고르세요(`kubectl get nodes -o custom-columns=NAME:.metadata.name,ARCH:.status.nodeInfo.architecture`).
- 이미 있는 태그는 덮어쓰지 않고 멈춥니다(`FORCE=1`로 강제). 같은 태그를 덮어쓰면 `imagePullPolicy: IfNotPresent`인 노드가 새 이미지를 받지 않으므로,
  master에서 다시 올릴 때는 `version`을 올리고, 다른 브랜치는 새 커밋에서 실행합니다.
- 태그는 커밋 기준이라 커밋하지 않은 서버 변경이 있으면 경고만 하고 그대로 빌드합니다. 배포용 이미지는 커밋한 뒤 올리세요.

손으로 하면 다음과 같습니다.

```bash
TAG=0.1.0
docker buildx build --platform linux/amd64,linux/arm64 -f k8s/docker/server.Dockerfile -t rouddy/cashbook:$TAG --push .
# kustomization.yaml의 newTag를 $TAG로
```

### CircleCI

**master 브랜치에 푸시할 때만** `.circleci/config.yml`이 같은 규칙(`docker/image_tag.sh`)으로 태그(`<version>`)를 정해,
Docker Hub에 그 태그가 **없을 때만** 빌드·푸시합니다(있으면 빌드하지 않고 성공 — 새 이미지는 `version`을 올려야 나옵니다). amd64·arm64를 각 아키텍처 머신에서 따로 빌드한 뒤 태그 하나로 묶으므로 QEMU보다 빠릅니다.
`kustomization.yaml`은 바꾸지 않으니 배포할 때 `newTag`를 직접 맞춥니다. 같은 워크플로에서 안드로이드 APK와 웹 빌드 zip도
아티팩트로 남깁니다. 필요한 프로젝트 환경 변수는 설정 파일 맨 위 주석에 있습니다(`DOCKERHUB_USERNAME`, `DOCKERHUB_TOKEN` 등).

Docker Hub 저장소를 **비공개**로 두면 클러스터가 받을 수 있게 Read-only 액세스 토큰으로 Secret을 만들고
`app/deployment.yaml`(웹 이미지 Job을 쓰면 `tools/web-sync-job.yaml`도)의 `imagePullSecrets` 주석을 풉니다.
이미지에는 서버 실행 파일만 있고 DB·비밀번호는 없습니다.

```bash
kubectl -n cashbook create secret docker-registry registry-credentials \
  --docker-server=https://index.docker.io/v1/ --docker-username=rouddy --docker-password='<Read-only 액세스 토큰>'
```

## 2. 네임스페이스와 PVC 만들기 (처음 한 번)

```bash
kubectl apply -f k8s/setup/namespace.yaml
kubectl apply -f k8s/setup/storage.yaml
kubectl -n cashbook get pvc        # local-path 등 WaitForFirstConsumer면 서버 Pod가 뜰 때까지 Pending이 정상
```

- 크기는 각 1Gi입니다(DB는 수 MB, 웹 빌드는 릴리스당 1MB 안팎). 필요하면 `storage.yaml`에서 바꿉니다.
- 특정 StorageClass를 쓰려면 `storageClassName` 주석을 풉니다. 비우면 클러스터 기본값입니다.

### 데이터 보존

PVC를 지우면 StorageClass의 `reclaimPolicy`(대부분 `Delete`)에 따라 **데이터도 지워집니다**.
`kubectl delete namespace cashbook`도 PVC를 함께 지웁니다. 실수로 지워지지 않게 PV를 Retain으로 바꿔 두세요.

```bash
for pv in $(kubectl -n cashbook get pvc cashbook-data cashbook-web -o jsonpath='{.items[*].spec.volumeName}'); do
  kubectl patch pv "$pv" -p '{"spec":{"persistentVolumeReclaimPolicy":"Retain"}}'
done
```

(PV는 PVC가 바인딩된 뒤, 즉 서버 Pod가 한 번 뜬 뒤에 생깁니다.)

## 3. Secret (비밀번호·DB 연결)

`cashbook-secret`에 두 값이 모두 있어야 합니다. 하나라도 없으면 Pod가 `CreateContainerConfigError`로 시작하지 않습니다.

| 키 | 값 |
|---|---|
| `CASHBOOK_PASSWORD` | 웹 로그인 비밀번호. 외부 주소(`0.0.0.0`)로 열려면 필수 |
| `DATABASE_URL` | SQLite: `sqlite:///data/cashbook.db?mode=rwc` (데이터 PVC, 스크립트가 이 경로를 씀)<br>MySQL: `mysql://사용자:비밀번호@호스트:3306/DB이름` (7장) |

```bash
# SQLite
kubectl -n cashbook create secret generic cashbook-secret \
  --from-literal=CASHBOOK_PASSWORD='긴-비밀번호' \
  --from-literal=DATABASE_URL='sqlite:///data/cashbook.db?mode=rwc'

# MySQL (비밀번호의 @ : / ? # % 공백 등은 URL 인코딩: python3 -c 'import sys,urllib.parse as u;print(u.quote(sys.argv[1],safe=""))' '비밀번호')
kubectl -n cashbook create secret generic cashbook-secret \
  --from-literal=CASHBOOK_PASSWORD='긴-비밀번호' \
  --from-literal=DATABASE_URL='mysql://cashbook:URL인코딩한-비밀번호@mysql.db.svc.cluster.local:3306/cashbook'
```

값을 바꿀 때는 같은 명령에 `--dry-run=client -o yaml | kubectl apply -f -`를 붙이고 `kubectl -n cashbook rollout restart deployment/cashbook`으로 다시 띄웁니다
(처음 한 번 나오는 `last-applied-configuration` 경고는 무시해도 됩니다).
비밀번호를 바꾸면 기존 로그인 세션은 무효가 되고, API 토큰은 그대로 씁니다.

세션 쿠키 암호화 키는 처음 시작할 때 `/data/secret.key`(데이터 PVC)에 만들어 두므로 재시작해도 로그인이 유지됩니다. MySQL을 써도 이 PVC는 그대로 둡니다.

## 4. HTTPS 준비 (cashbook.ster.pe.kr)

공개 주소는 `https://cashbook.ster.pe.kr`입니다. Ingress(Traefik)에서 TLS가 끝나고, 인증서는 cert-manager가
Let's Encrypt에서 받아 `cashbook-tls` Secret에 넣고 만료 전에 자동으로 갱신합니다.

### 4-1. 네트워크: 공인 IP의 80·443이 클러스터 Ingress로 와야 한다

DNS는 `cashbook.ster.pe.kr → CNAME localgo.duckdns.org → 공인 IP`로 잡혀 있습니다.
Let's Encrypt는 `http://cashbook.ster.pe.kr/.well-known/acme-challenge/...`(**80**)로 도메인을 확인하고 브라우저는 **443**으로 접속하므로,
두 포트가 k3s 노드(192.168.0.99)의 Traefik(servicelb가 노드의 80·443에서 엶)으로 가야 합니다.

2026-11-04에 밖에서 확인했을 때는 80에서 다른 nginx(nginx/1.24.0, "Local Services" 페이지)가 모든 도메인에 응답했고,
443은 TLS 연결을 바로 끊었습니다. 다음 중 하나로 맞춥니다.

- **A. 공유기 포트 포워딩**: 80·443 → 192.168.0.99:80·443. 기존 nginx로 열던 서비스는 Traefik Ingress로 옮기거나 다른 포트로.
- **B. 기존 nginx를 앞에 두고 이 도메인만 넘기기**: 80은 HTTP 프록시, 443은 **TLS를 풀지 않고** SNI로 그대로 Traefik에 넘깁니다.
  (앞단 nginx에서 TLS를 풀고 HTTP로 넘기면 Traefik이 HTTP로 보고 다시 HTTPS로 보내 **무한 리다이렉트**가 됩니다.)

```nginx
# B. /etc/nginx/nginx.conf 최상위 (http {} 밖). Ubuntu는 libnginx-mod-stream 필요
stream {
    map $ssl_preread_server_name $tls_upstream {
        cashbook.ster.pe.kr  192.168.0.99:443;
        default              127.0.0.1:8443;    # 이 nginx의 다른 https 사이트가 있다면 listen 8443 ssl로 옮긴다
    }
    server {
        listen 443;
        ssl_preread on;
        proxy_pass $tls_upstream;
    }
}

# B. http {} 안 (sites-available 등): 80은 이 도메인만 Traefik으로 (인증서 확인 + HTTPS 리다이렉트)
server {
    listen 80;
    server_name cashbook.ster.pe.kr;
    location / {
        proxy_pass http://192.168.0.99;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    }
}
```

밖에서 확인: `curl -I http://cashbook.ster.pe.kr` 응답이 Traefik(앱 배포 후 `301 → https://...`)이어야 합니다.
nginx 1.24 "Local Services" 페이지가 나오면 아직 80이 클러스터로 오지 않는 것입니다.

### 4-2. cert-manager와 ClusterIssuer (클러스터에 한 번)

```bash
# cert-manager가 없으면 설치 (이미 있으면 건너뜀)
kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.21.3/cert-manager.yaml
kubectl -n cert-manager rollout status deployment/cert-manager-webhook

kubectl apply -f k8s/setup/cluster-issuer.yaml
kubectl get clusterissuer        # letsencrypt-staging, letsencrypt-prod 모두 READY True
```

만료 알림 메일을 받으려면 `cluster-issuer.yaml`의 `acme`에 `email:`을 넣습니다(선택).

### 4-3. Ingress 설정 (k8s/app)

| 파일 | 내용 |
|---|---|
| `ingress.yaml` | host·TLS `cashbook.ster.pe.kr`, `cert-manager.io/cluster-issuer: letsencrypt-prod` → Secret `cashbook-tls` |
| `traefik/` (component) | HTTP → HTTPS 301 리다이렉트 (Middleware `redirect-https`). Let's Encrypt 확인 요청은 cert-manager가 만드는 별도 Ingress로 가서 리다이렉트되지 않음 |
| `configmap.yaml` | `CASHBOOK_ALLOWED_ORIGINS=https://cashbook.ster.pe.kr`, `CASHBOOK_SECURE_COOKIE=1` |

`ingressClassName`은 비워 두어 클러스터 기본 IngressClass(k3s는 traefik)를 씁니다.
**ingress-nginx**면 `kustomization.yaml`의 `components: - traefik`을 지우고 `ingress.yaml`의 `ingressClassName: nginx`를 켭니다
(리다이렉트는 `force-ssl-redirect` 주석이 처리).

## 5. 앱 배포

```bash
kubectl apply -k k8s/app
kubectl -n cashbook rollout status deployment/cashbook
kubectl -n cashbook logs deployment/cashbook

# 인증서: 보통 1~2분 안에 READY True
kubectl -n cashbook get certificate cashbook-tls
kubectl -n cashbook describe challenge          # 오래 걸리면 확인 실패 이유가 여기 나온다

curl -I http://cashbook.ster.pe.kr               # 301 → https://cashbook.ster.pe.kr/
curl https://cashbook.ster.pe.kr/api/health      # {"ok":true,"version":"..."}
```

설정을 처음 맞출 때는 staging으로 발급 경로를 먼저 확인하면 Let's Encrypt 실패 횟수 제한에 걸리지 않습니다.

```bash
kubectl -n cashbook annotate ingress cashbook cert-manager.io/cluster-issuer=letsencrypt-staging --overwrite
kubectl -n cashbook get certificate cashbook-tls -w      # READY True 확인 (브라우저는 신뢰하지 않는 인증서)
kubectl apply -k k8s/app                                 # 주석을 prod로 되돌림
kubectl -n cashbook delete secret cashbook-tls           # prod 인증서로 다시 발급
```

처음에는 DB가 비어 있고(마이그레이션만 적용) 웹 PVC도 비어 있어서 `/`에 "웹 화면이 아직 빌드되지 않았습니다"가 나옵니다.
아래 6·7로 웹 빌드와 기존 데이터를 올립니다. 자동 입력(안드로이드 앱·아이폰 단축어)의 서버 주소는 `https://cashbook.ster.pe.kr`입니다.

Ingress 없이 확인: `kubectl -n cashbook port-forward svc/cashbook 8080:80` → http://localhost:8080

## 6. 웹 빌드를 PVC에 올리기

`cashbook-web` PVC는 릴리스별 디렉터리와 현재 릴리스를 가리키는 링크로 되어 있고, 서버는 `CASHBOOK_WEB_DIR=/web/current`를 서빙합니다.

```
/web/releases/20261103-101500-5ed0dae/   index.html, assets/...
/web/releases/20261101-090000-26de940/
/web/current -> releases/20261103-101500-5ed0dae
```

릴리스 이름은 UTC 시각으로 시작해 이름순이 곧 시간순입니다(스크립트는 뒤에 git 커밋, Job은 `RELEASE` 값).
새 릴리스를 다 올린 뒤에 `current` 링크를 rename 한 번으로 바꾸므로, 올리는 도중에도 이전 화면이 그대로 나오고
서버를 재시작할 필요가 없습니다. 이전 릴리스는 최근 3개까지 남겨 롤백에 씁니다.

### 방법 1: 로컬에서 빌드해 올리기 (기본)

```bash
k8s/scripts/deploy-web.sh                 # web에서 npm ci && npm run build → 업로드 → current 교체
SKIP_BUILD=1 k8s/scripts/deploy-web.sh    # 이미 빌드한 web/dist를 올리기
KEEP_RELEASES=5 k8s/scripts/deploy-web.sh
```

스크립트는 서버 이미지로 작업용 Pod(`cashbook-pvc-shell`)를 서버와 같은 노드에 띄워 PVC를 마운트하고,
`tar`로 스트리밍해 올린 뒤 Pod를 지웁니다. 손으로 하면 다음과 같습니다.

```bash
IMAGE=$(kubectl -n cashbook get deploy cashbook -o jsonpath='{.spec.template.spec.containers[0].image}')
sed "s#image: cashbook-server#image: $IMAGE#" k8s/tools/pvc-shell.yaml | kubectl apply -f -
kubectl -n cashbook wait --for=condition=Ready pod/cashbook-pvc-shell

R=$(date -u +%Y%m%d-%H%M%S)
tar -C web/dist -cf - . | kubectl -n cashbook exec -i cashbook-pvc-shell -- \
  sh -c "mkdir -p /web/releases/$R && tar -xf - -C /web/releases/$R"
kubectl -n cashbook exec cashbook-pvc-shell -- \
  sh -c "cd /web && ln -sfn releases/$R current.new && mv -Tf current.new current"

kubectl -n cashbook delete pod cashbook-pvc-shell
```

### 방법 2: 웹 이미지 + Job (CI·레지스트리로 배포)

웹 이미지는 같은 Docker Hub 저장소에 `web-` 태그로 올립니다.

```bash
TAG=0.1.0
docker buildx build --platform linux/amd64,linux/arm64 -f k8s/docker/web.Dockerfile -t rouddy/cashbook:web-$TAG --push .
```

`k8s/tools/web-sync-job.yaml`의 `image`(`rouddy/cashbook:web-$TAG`)와 `RELEASE`를 이 태그로 바꾸고 실행합니다(이름이 매번 새로 붙도록 `create`).

```bash
kubectl create -f k8s/tools/web-sync-job.yaml
kubectl -n cashbook logs -f job/$(kubectl -n cashbook get job -l app.kubernetes.io/component=web-sync \
  --sort-by=.metadata.creationTimestamp -o jsonpath='{.items[-1].metadata.name}')
```

끝난 Job은 1시간 뒤 자동으로 지워집니다.

### 롤백

```bash
kubectl -n cashbook exec deploy/cashbook -- ls -1r /web/releases     # 남은 릴리스 (이름 = 시각, 최신순)

# 서버 Pod는 /web을 읽기 전용으로 마운트하므로, 위 "손으로 하면"처럼 작업용 Pod를 띄워 링크만 되돌린다
kubectl -n cashbook exec cashbook-pvc-shell -- sh -c "cd /web && ln -sfn releases/<릴리스> current.new && mv -Tf current.new current"
kubectl -n cashbook delete pod cashbook-pvc-shell
```

## 7. 데이터베이스 (SQLite / MySQL)

서버는 시작할 때 마이그레이션을 적용하므로 SQLite든 MySQL이든 빈 DB를 주면 테이블을 만듭니다.

### 왜 replicas 1, Recreate인가

SQLite는 한 프로세스가 쓰는 것을 전제로 합니다. MySQL이어도 로그인 실패 제한이 Pod 메모리에 있고 세션 키가 RWO PVC에 있어 1개로 둡니다. Deployment는 항상 1개이고, 업데이트할 때 기존 Pod를 먼저 내린 뒤 새 Pod를 띄웁니다
(`strategy: Recreate`). 그래서 업데이트 중 몇 초간 접속이 끊깁니다. HPA나 replicas 증가를 적용하지 마세요.
DB는 기본 저널 모드(delete)라 쓰는 동안 `/data/cashbook.db-journal`이 잠깐 생겼다 사라집니다.

### SQLite: 기존 로컬 DB 옮기기 / 백업에서 되돌리기

```bash
# 로컬 DB를 일관된 한 파일로 (서버가 실행 중이어도 안전)
sqlite3 server/data/cashbook.db ".backup /tmp/cashbook.db"

k8s/scripts/restore-db.sh /tmp/cashbook.db
```

`restore-db.sh`는 무결성을 확인하고, 서버를 0개로 내린 뒤 작업용 Pod로 파일을 올리고, 기존 DB를
`/data/backups/before-restore-<시각>.db`로 남긴 다음 바꾸고 서버를 다시 띄웁니다(중간에 실패해도 서버는 다시 띄움).
`secret.key`는 옮기지 않아도 됩니다(새 키로 한 번 다시 로그인). API 토큰은 DB에 있으므로 그대로 쓸 수 있습니다.

스프레드시트 변환 JSON을 넣을 때는 서버 Pod 안에서 가져오기 명령을 씁니다.

```bash
POD=$(kubectl -n cashbook get pod -l app.kubernetes.io/component=server -o jsonpath='{.items[0].metadata.name}')
kubectl -n cashbook cp import/normalized.json $POD:/tmp/normalized.json
kubectl -n cashbook exec $POD -- cashbook import /tmp/normalized.json      # --replace는 기존 데이터를 지움 ⚠️
kubectl -n cashbook exec $POD -- rm /tmp/normalized.json
```

### 백업

```bash
k8s/scripts/backup-db.sh                        # ./cashbook-backup-<시각>.db
k8s/scripts/backup-db.sh ~/backups/cash.db
```

실행 중인 서버 Pod에서 한 시점의 사본을 떠서 무결성을 확인한 뒤 SQLite 파일로 받아 옵니다. 서버를 멈추지 않습니다.
- SQLite: `sqlite3 .backup`(온라인 백업)
- MySQL: `cashbook copy-db`가 한 트랜잭션 스냅샷을 SQLite 파일로 복사. 이 파일은 `copy-db`로 SQLite·MySQL 어느 쪽으로든 되돌릴 수 있습니다.
  (`mysqldump --single-transaction`으로 따로 백업해도 됩니다)

PVC 안의 `/data/backups`(복원 전 백업)는 같은 디스크에 있으므로 정기적으로 클러스터 밖에 백업을 받아 두세요
(cron에서 `backup-db.sh` 실행, 또는 Velero 같은 볼륨 백업).

### MySQL 쓰기

MySQL 8.x(8.4에서 확인)에 빈 DB와 사용자를 만듭니다. 정렬 규칙은 **`utf8mb4_bin`** 을 권장합니다 —
SQLite와 같이 대소문자를 구분하고 이름순 정렬도 같습니다(`_ci` 정렬이면 "Food"와 "food"가 같은 이름으로 취급됩니다).

```sql
CREATE DATABASE cashbook CHARACTER SET utf8mb4 COLLATE utf8mb4_bin;
CREATE USER 'cashbook'@'%' IDENTIFIED BY '긴-비밀번호';
GRANT ALL PRIVILEGES ON cashbook.* TO 'cashbook'@'%';   -- 시작할 때 테이블 생성·변경(마이그레이션)
```

- `DATABASE_URL`: `mysql://cashbook:<URL 인코딩한 비밀번호>@<호스트>:3306/cashbook`. 클러스터 안 MySQL이면 `<서비스>.<네임스페이스>.svc.cluster.local`.
  TLS를 강제하려면 끝에 `?ssl-mode=required`.
- 서버 Pod에서 MySQL로 나가는 연결이 막혀 있지 않아야 합니다(NetworkPolicy·방화벽).
- MySQL에서 달라지는 점: 시각(`createdAt` 등)은 초 단위까지 저장, 거래 내용(description)은 255자까지(자동 입력 원문·메모는 길이 제한 없음).

#### SQLite → MySQL 옮기기

서버를 내리고 Secret을 MySQL로 바꾼 뒤, 작업용 Pod(서버 이미지, 데이터 PVC와 `DATABASE_URL`을 받음)에서 `copy-db`로 복사합니다.
DB 비밀번호는 Secret에서 환경 변수로만 전달되어 명령줄에 남지 않습니다. 대상 MySQL DB는 비어 있어야 합니다.

```bash
k8s/scripts/backup-db.sh before-mysql.db                       # 만약을 위해
kubectl -n cashbook scale deployment/cashbook --replicas=0

kubectl -n cashbook create secret generic cashbook-secret \
  --from-literal=CASHBOOK_PASSWORD='긴-비밀번호' \
  --from-literal=DATABASE_URL='mysql://cashbook:...@mysql.db.svc.cluster.local:3306/cashbook' \
  --dry-run=client -o yaml | kubectl apply -f -

IMAGE=$(kubectl -n cashbook get deploy cashbook -o jsonpath='{.spec.template.spec.containers[0].image}')
sed "s#image: cashbook-server#image: $IMAGE#" k8s/tools/pvc-shell.yaml | kubectl apply -f -
kubectl -n cashbook wait --for=condition=Ready pod/cashbook-pvc-shell
kubectl -n cashbook exec cashbook-pvc-shell -- \
  sh -c 'cashbook copy-db "sqlite:///data/cashbook.db?mode=rwc" "$DATABASE_URL"'
kubectl -n cashbook delete pod cashbook-pvc-shell

kubectl -n cashbook scale deployment/cashbook --replicas=1
kubectl -n cashbook rollout status deployment/cashbook
```

`copy-db`는 테이블을 외래 키 순서대로 id 그대로 옮기고(한 트랜잭션, 실패하면 대상에 아무것도 남지 않음), 테이블별 행 수를 출력합니다.
SQLite 파일(`/data/cashbook.db`)은 지우지 않으므로 문제가 있으면 Secret을 SQLite로 되돌리면 됩니다.
반대로 MySQL → SQLite도 같은 방법입니다(`copy-db "$DATABASE_URL" "sqlite:///data/new.db?mode=rwc"` 후 Secret을 그 경로로, 기존 파일과 겹치지 않게).

#### MySQL 백업에서 되돌리기

`restore-db.sh`는 SQLite 전용입니다. MySQL은 DB를 비운 뒤(`DROP DATABASE`/`CREATE DATABASE`) 위와 같이 작업용 Pod에서
`backup-db.sh`로 받은 SQLite 파일을 `kubectl cp`로 `/data`에 올리고 `copy-db "sqlite:///data/<파일>?mode=rwc" "$DATABASE_URL"`로 넣습니다
(또는 `mysqldump`로 받은 SQL을 `mysql`로 복원).

## 8. 업데이트

```bash
# 서버: 새 이미지 푸시 + kustomization.yaml newTag 변경 → 적용 (Recreate로 잠깐 끊김, 마이그레이션은 시작할 때 자동)
k8s/docker/build_and_push.sh
kubectl apply -k k8s/app

# 웹: 서버 재시작 없이
k8s/scripts/deploy-web.sh
```

서버 업데이트 전에 `backup-db.sh`로 백업을 받아 두면 마이그레이션 문제가 생겨도 되돌릴 수 있습니다(SQLite는 `restore-db.sh`, MySQL은 7장).

## 설정

`app/configmap.yaml` (값을 바꾸면 `kubectl apply -k k8s/app && kubectl -n cashbook rollout restart deployment/cashbook`)

| 변수 | 값 | 설명 |
|---|---|---|
| `CASHBOOK_SECRET_FILE` | `/data/secret.key` | 세션 쿠키 키 (없으면 생성) |
| `CASHBOOK_WEB_DIR` | `/web/current` | 웹 빌드 PVC의 현재 릴리스 |
| `CASHBOOK_TZ` | `Asia/Seoul` | "오늘" 기준 |
| `CASHBOOK_PLAN_MONTHS` | `2` | 반복 규칙 예정 거래 개월 수 |
| `CASHBOOK_SECURE_COOKIE` | `1` | HTTPS Ingress 뒤 |
| `CASHBOOK_ALLOWED_ORIGINS` | `https://cashbook.ster.pe.kr` | 브라우저 요청 Origin 허용 (앞단 프록시가 Host를 바꿔도 통과) |
| `ROCKET_ADDRESS`, `ROCKET_PORT` | `0.0.0.0`, `8000` | |
| `CASHBOOK_PASSWORD` | Secret `cashbook-secret` | 필수 |
| `DATABASE_URL` | Secret `cashbook-secret` | 필수. `sqlite:///data/cashbook.db?mode=rwc` 또는 `mysql://...` (3장) |

나머지 변수와 의미는 [루트 README의 설정](../README.md#설정-환경-변수).

보안 관련 동작
- 서버는 브라우저 요청의 `Origin`이 `Host`와 다르면 403으로 막습니다. Ingress가 Host를 그대로 넘기면 문제없고,
  바꾸는 구성이면 `X-Forwarded-Host`를 넘기거나 `CASHBOOK_ALLOWED_ORIGINS`에 공개 주소를 넣습니다.
- 로그인 실패 제한은 `X-Real-IP` 헤더(ingress-nginx·Traefik이 붙임)로 클라이언트를 구분합니다.
  servicelb나 앞단 nginx(stream)를 거치면 Traefik이 실제 클라이언트 IP 대신 중간 주소를 보게 되어 모든 접속이 한 IP로 묶입니다
  (그래도 동작은 하지만 다른 곳의 실패가 내 로그인을 10분 막을 수 있음). 실제 IP가 필요하면 Traefik Service의
  `externalTrafficPolicy: Local`, 앞단 nginx면 PROXY protocol(`proxy_protocol on` + Traefik `proxyProtocol.trustedIPs`)을 씁니다.
- Pod는 root가 아닌 uid 10001, 읽기 전용 루트 파일시스템, 모든 capability 제거로 실행합니다. 쓰는 곳은 `/data`, `/tmp`뿐입니다.
- 이미지에는 DB·비밀번호가 들어가지 않습니다(비밀번호·DB 연결 정보는 Secret, 데이터는 PVC 또는 MySQL).

## 문제 해결

| 증상 | 확인 |
|---|---|
| Pod `Pending` | `kubectl -n cashbook describe pod` — PVC가 Bound인지, StorageClass가 있는지 |
| `CreateContainerConfigError` | Secret `cashbook-secret` 또는 키 `CASHBOOK_PASSWORD`·`DATABASE_URL`이 없음 (3장) |
| `CrashLoopBackOff`, 로그에 DB 연결 오류 | `DATABASE_URL`(호스트·포트·사용자·URL 인코딩한 비밀번호), MySQL 권한(`GRANT ALL ON cashbook.*`), 네트워크 |
| `ImagePullBackOff` | 이미지 이름·태그(`docker buildx imagetools inspect rouddy/cashbook:<태그>`), 비공개 저장소면 `imagePullSecrets`, 노드 아키텍처(amd64/arm64) |
| 작업용 Pod·Job이 `ContainerCreating`에서 멈춤, `Multi-Attach error` | RWO 볼륨이 다른 노드의 서버에 붙어 있음. 같은 노드에 뜨도록 podAffinity가 있지만 노드 자원이 부족하면 실패 — 서버를 잠시 0개로 내린 뒤 다시 |
| `cashbook-tls` 인증서가 READY가 안 됨 | `kubectl -n cashbook describe challenge` — 80번이 클러스터로 오지 않음(4-1), DNS, ClusterIssuer READY |
| 브라우저 인증서 경고 | 아직 발급 전(Traefik 기본 인증서)이거나 staging 인증서 → prod로 다시 발급 (5장) |
| `ERR_TOO_MANY_REDIRECTS` | 앞단 프록시가 TLS를 풀고 HTTP로 넘김 → 443은 SNI 패스스루로 (4-1 B) |
| `/`에 "웹 화면이 아직 빌드되지 않았습니다" | 웹 PVC가 비었거나 `current` 링크가 없음 → `deploy-web.sh` |
| 로그인은 되는데 저장(API)이 403 | Origin/Host 불일치 → 위 보안 동작 참고 |
| 로그인 후 바로 풀림 | HTTP로 접속 중인데 `CASHBOOK_SECURE_COOKIE=1` (Secure 쿠키는 HTTPS에서만). port-forward로 볼 때는 `localhost` 주소 사용 |
| 저장(API)이 403 "다른 사이트에서 보낸 요청" | 접속한 주소가 `CASHBOOK_ALLOWED_ORIGINS`·Host와 다름 (다른 도메인으로도 열면 쉼표로 추가) |

## 지우기

```bash
kubectl delete -k k8s/app          # 앱만 (PVC·Secret·데이터는 남음)
# ⚠️ 데이터까지 지우려면 백업을 받은 뒤
kubectl -n cashbook delete pvc cashbook-data cashbook-web
kubectl delete namespace cashbook
```
