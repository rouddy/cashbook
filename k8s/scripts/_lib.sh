# k8s/scripts 공통. 클러스터는 현재 kubectl 컨텍스트(KUBECONFIG, --context는 환경 변수 KUBECTL로 바꿀 수 있음).
#   NAMESPACE=cashbook            대상 네임스페이스
#   KUBECTL="kubectl --context x" 사용할 kubectl 명령

NS="${NAMESPACE:-cashbook}"
read -r -a KUBECTL_CMD <<<"${KUBECTL:-kubectl}"
SCRIPTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
K8S_DIR="$(dirname "$SCRIPTS_DIR")"
REPO_DIR="$(dirname "$K8S_DIR")"
SHELL_POD=cashbook-pvc-shell
SERVER_SELECTOR="app.kubernetes.io/name=cashbook,app.kubernetes.io/component=server"

kube() { "${KUBECTL_CMD[@]}" -n "$NS" "$@"; }

die() {
  echo "오류: $*" >&2
  exit 1
}

# 배포된 서버 이미지 (도구 Pod도 같은 이미지를 쓴다)
server_image() {
  local image
  image="$(kube get deployment cashbook -o jsonpath='{.spec.template.spec.containers[?(@.name=="server")].image}' 2>/dev/null)" || true
  [[ -n "$image" ]] || die "네임스페이스 $NS에 cashbook Deployment가 없습니다. 먼저 kubectl apply -k k8s/app"
  echo "$image"
}

# DB 종류: Secret cashbook-secret의 DATABASE_URL 앞부분 (sqlite / mysql). 값 자체는 출력하지 않는다.
db_kind() {
  local url
  url="$(kube get secret cashbook-secret -o jsonpath='{.data.DATABASE_URL}' 2>/dev/null | base64 --decode 2>/dev/null || true)"
  case "$url" in
    sqlite:*) echo sqlite ;;
    mysql:*) echo mysql ;;
    "") echo missing ;;
    *) echo other ;;
  esac
}

# SQLite 파일을 다루는 스크립트용: Secret의 DATABASE_URL이 SQLite가 아니면 멈춘다
require_sqlite() {
  local kind
  kind="$(db_kind)"
  case "$kind" in
    sqlite) ;;
    mysql) die "DATABASE_URL이 MySQL입니다. 이 스크립트는 SQLite(데이터 PVC) 전용입니다 — MySQL은 k8s/README.md \"MySQL 백업에서 되돌리기\"를 보세요" ;;
    missing) die "Secret cashbook-secret에 DATABASE_URL이 없습니다 (k8s/README.md 3장)" ;;
    *) die "DATABASE_URL 형식을 알 수 없습니다" ;;
  esac
}

server_pod() {
  kube get pod -l "$SERVER_SELECTOR" --field-selector=status.phase=Running \
    -o jsonpath='{.items[0].metadata.name}' 2>/dev/null || true
}

# 두 PVC를 마운트한 작업용 Pod를 띄우고 준비될 때까지 기다린다
start_shell() {
  local image
  image="$(server_image)"
  kube delete pod "$SHELL_POD" --ignore-not-found --wait=true >/dev/null
  sed -e "s#image: cashbook-server#image: ${image}#" -e "s#namespace: cashbook#namespace: ${NS}#" \
    "$K8S_DIR/tools/pvc-shell.yaml" | kube apply -f - >/dev/null
  if ! kube wait --for=condition=Ready "pod/$SHELL_POD" --timeout=180s >/dev/null; then
    kube describe pod "$SHELL_POD" | tail -n 15 >&2
    die "작업용 Pod가 준비되지 않았습니다 (PVC가 다른 노드에 붙어 있으면 Multi-Attach 오류가 납니다)"
  fi
}

stop_shell() {
  kube delete pod "$SHELL_POD" --ignore-not-found --wait=false >/dev/null 2>&1 || true
}
