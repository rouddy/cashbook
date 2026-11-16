#!/usr/bin/env bash
# 서버 이미지를 빌드해 Docker Hub(rouddy/cashbook)에 푸시하고 k8s/app/kustomization.yaml의 태그를 바꾼다.
# 클러스터에는 손대지 않는다 — 푸시한 뒤 kubectl apply -k k8s/app 으로 배포.
#
# 태그 (version = server/Cargo.toml [package]의 version)
#   master 브랜치        <version>          예: 0.1.0
#   그 밖 (detached 포함) <version>-<커밋>   예: 0.1.0-5ed0dae
#
#   k8s/docker/build_and_push.sh
#   PLATFORMS=linux/arm64 k8s/docker/build_and_push.sh   # 노드 아키텍처만 (기본 amd64+arm64)
#   FORCE=1 k8s/docker/build_and_push.sh                 # 이미 있는 태그를 덮어쓰기
#
# 먼저 docker login -u rouddy (비밀번호 대신 Read & Write 액세스 토큰 권장).
set -euo pipefail

DOCKER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$DOCKER_DIR/../.." && pwd)"
IMAGE="${IMAGE:-rouddy/cashbook}"
# 빌드 PC와 다른 아키텍처는 QEMU 에뮬레이션으로 Rust를 컴파일하므로 오래 걸린다
PLATFORMS="${PLATFORMS:-linux/amd64,linux/arm64}"
KUSTOMIZATION="$REPO_DIR/k8s/app/kustomization.yaml"

die() {
  echo "오류: $*" >&2
  exit 1
}

# [package] 섹션의 version (의존성 테이블의 version은 건너뛴다)
version="$(awk -F'"' '/^\[/ { pkg = ($0 == "[package]") } pkg && /^version[[:space:]]*=/ { print $2; exit }' \
  "$REPO_DIR/server/Cargo.toml")"
[[ -n "$version" ]] || die "server/Cargo.toml [package]에서 version을 찾지 못했습니다"

branch="$(git -C "$REPO_DIR" symbolic-ref --quiet --short HEAD || true)" # detached HEAD면 비어 있음
sha="$(git -C "$REPO_DIR" rev-parse --short HEAD)"
if [[ "$branch" == master ]]; then
  TAG="$version"
else
  TAG="$version-$sha"
fi
[[ "$TAG" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]] || die "Docker 태그로 쓸 수 없는 값입니다: $TAG"

if [[ -n "$(git -C "$REPO_DIR" status --porcelain -- server/Cargo.toml server/Cargo.lock server/Rocket.toml server/src \
  k8s/docker/server.Dockerfile k8s/docker/server.Dockerfile.dockerignore)" ]]; then
  echo "주의: 커밋하지 않은 서버 변경이 이미지에 들어갑니다 (태그는 커밋 $sha 기준)" >&2
fi

# 이미 있는 태그를 덮어쓰면 imagePullPolicy: IfNotPresent인 노드는 새 이미지를 받지 않는다
if [[ "${FORCE:-0}" != 1 ]] && docker buildx imagetools inspect "$IMAGE:$TAG" >/dev/null 2>&1; then
  if [[ "$branch" == master ]]; then
    die "$IMAGE:$TAG 가 이미 있습니다. server/Cargo.toml의 version을 올리세요 (덮어쓰려면 FORCE=1)"
  fi
  die "$IMAGE:$TAG 가 이미 있습니다. 새 커밋에서 다시 실행하세요 (덮어쓰려면 FORCE=1)"
fi

echo "브랜치 ${branch:-(detached)} → $IMAGE:$TAG ($PLATFORMS)"
# 빌드 컨텍스트는 저장소 루트지만 server.Dockerfile.dockerignore(허용 목록)로 서버 소스만 들어간다
docker buildx build \
  --platform "$PLATFORMS" \
  -f "$DOCKER_DIR/server.Dockerfile" \
  -t "$IMAGE:$TAG" \
  --push \
  "$REPO_DIR"

# images 항목은 cashbook-server 하나뿐이다
sed -i.bak \
  -e "s#^\([[:space:]]*newName:\).*#\1 $IMAGE#" \
  -e "s#^\([[:space:]]*newTag:\).*#\1 \"$TAG\"#" \
  "$KUSTOMIZATION"
rm -f "$KUSTOMIZATION.bak"

echo
echo "푸시 완료: $IMAGE:$TAG"
echo "k8s/app/kustomization.yaml 태그 변경 → 배포: kubectl apply -k k8s/app && kubectl -n cashbook rollout status deployment/cashbook"
