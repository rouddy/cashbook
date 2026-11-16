#!/usr/bin/env bash
# 웹 빌드(web/dist)를 cashbook-web PVC에 새 릴리스로 올리고 /web/current 링크를 바꾼다.
# 서버는 요청마다 파일을 읽으므로 재시작하지 않아도 바로 새 화면이 나온다.
#
#   k8s/scripts/deploy-web.sh                 # web에서 npm ci && npm run build 후 업로드
#   SKIP_BUILD=1 k8s/scripts/deploy-web.sh    # 이미 빌드된 web/dist를 업로드
#   KEEP_RELEASES=5 WEB_DIST=/path/dist k8s/scripts/deploy-web.sh
set -euo pipefail
source "$(dirname "$0")/_lib.sh"

DIST="${WEB_DIST:-$REPO_DIR/web/dist}"
KEEP="${KEEP_RELEASES:-3}"
[[ "$KEEP" =~ ^[1-9][0-9]*$ ]] || die "KEEP_RELEASES는 1 이상의 정수여야 합니다"

if [[ "${SKIP_BUILD:-0}" != 1 ]]; then
  (cd "$REPO_DIR/web" && npm ci && npm run build)
fi
[[ -f "$DIST/index.html" ]] || die "$DIST/index.html이 없습니다 (web에서 npm run build)"

rev="$(git -C "$REPO_DIR" rev-parse --short HEAD 2>/dev/null || true)"
release="$(date -u +%Y%m%d-%H%M%S)${rev:+-$rev}"  # UTC: Job과 같은 기준으로 이름순 = 시간순

start_shell
trap stop_shell EXIT

echo "업로드: $DIST → cashbook-web:/web/releases/$release"
# 다 받은 뒤에 이름을 바꿔야 반쯤 올라간 릴리스를 가리키지 않는다. (COPYFILE_DISABLE: macOS의 ._ 파일 제외)
COPYFILE_DISABLE=1 tar -C "$DIST" -cf - . |
  kube exec -i "$SHELL_POD" -- sh -euc "
    mkdir -p /web/releases/.incoming-$release
    tar --warning=no-unknown-keyword -xf - -C /web/releases/.incoming-$release
    mv /web/releases/.incoming-$release /web/releases/$release"

kube exec "$SHELL_POD" -- sh -euc "
  cd /web
  ln -sfn releases/$release current.new
  mv -Tf current.new current
  cd releases
  # 이름이 시각으로 시작하므로 이름 역순 = 최신순 (tar가 디렉터리 mtime을 로컬 값으로 맞추므로 ls -t는 쓰지 않는다)
  ls -1 | sort -r | grep -vx '$release' | tail -n +$KEEP | xargs -r rm -rf
  find . -maxdepth 1 -name '.incoming-*' ! -name '.incoming-$release' -exec rm -rf {} +
  echo \"current -> \$(readlink /web/current)\"
  echo '남은 릴리스:'; ls -1 | sort -r"
