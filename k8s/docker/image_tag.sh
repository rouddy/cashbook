# 서버 이미지 태그를 정한다. 실행하지 않고 source해서 쓴다 (build_and_push.sh, .circleci/config.yml).
#
#   master 브랜치        <version>          예: 0.1.0
#   그 밖 (detached 포함) <version>-<커밋>   예: 0.1.0-5ed0dae
#   (version = server/Cargo.toml [package]의 version)
#
# 입력: REPO_DIR (없으면 이 파일 기준 저장소 루트)
#       BRANCH   (없으면 git에서 읽는다 — CI처럼 detached HEAD로 받을 때 지정)
# 결과: version, branch, sha, TAG

REPO_DIR="${REPO_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"

# [package] 섹션의 version (의존성 테이블의 version은 건너뛴다)
version="$(awk -F'"' '/^\[/ { pkg = ($0 == "[package]") } pkg && /^version[[:space:]]*=/ { print $2; exit }' \
  "$REPO_DIR/server/Cargo.toml")"
if [[ -z "$version" ]]; then
  echo "오류: server/Cargo.toml [package]에서 version을 찾지 못했습니다" >&2
  return 1
fi

branch="${BRANCH:-$(git -C "$REPO_DIR" symbolic-ref --quiet --short HEAD || true)}" # detached HEAD면 비어 있음
sha="$(git -C "$REPO_DIR" rev-parse --short HEAD)"
if [[ "$branch" == master ]]; then
  TAG="$version"
else
  TAG="$version-$sha"
fi
if ! [[ "$TAG" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
  echo "오류: Docker 태그로 쓸 수 없는 값입니다: $TAG" >&2
  return 1
fi
