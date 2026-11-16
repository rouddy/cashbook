#!/usr/bin/env bash
# 로컬 SQLite 파일로 클러스터의 DB를 바꾼다 (기존 로컬 DB 옮기기, 백업에서 되돌리기).
# 서버를 잠시 0개로 내리고, 기존 DB는 PVC의 /data/backups/before-restore-<시각>.db로 남긴 뒤 바꾼다.
#
#   sqlite3 server/data/cashbook.db ".backup /tmp/cashbook.db"   # 실행 중인 로컬 DB도 안전하게 한 파일로
#   k8s/scripts/restore-db.sh /tmp/cashbook.db
#   YES=1 k8s/scripts/restore-db.sh backup.db                      # 확인 질문 없이
set -euo pipefail
source "$(dirname "$0")/_lib.sh"

src="${1:-}"
[[ -n "$src" && -f "$src" ]] || die "복원할 SQLite 파일을 지정하세요: $0 <file.db>"
if command -v sqlite3 >/dev/null; then
  [[ "$(sqlite3 "$src" "PRAGMA integrity_check")" == ok ]] || die "$src 무결성 확인 실패"
fi
server_image >/dev/null
require_sqlite

if [[ "${YES:-0}" != 1 ]]; then
  read -r -p "네임스페이스 $NS의 서버를 잠시 멈추고 DB를 $src로 바꿉니다. 계속할까요? [y/N] " answer
  [[ "$answer" == [yY] ]] || exit 1
fi

ts="$(date +%Y%m%d-%H%M%S)"
restore_server() {
  stop_shell
  kube scale deployment/cashbook --replicas=1 >/dev/null
}
trap restore_server EXIT

echo "서버 내리는 중..."
kube scale deployment/cashbook --replicas=0 >/dev/null
kube wait --for=delete pod -l "$SERVER_SELECTOR" --timeout=180s >/dev/null 2>&1 || true
[[ -z "$(kube get pod -l "$SERVER_SELECTOR" -o name)" ]] || die "서버 Pod가 아직 남아 있습니다"

start_shell
kube cp "$src" "$SHELL_POD:/data/.incoming.db" >/dev/null
kube exec "$SHELL_POD" -- sh -euc "
  cd /data
  check=\$(sqlite3 .incoming.db 'PRAGMA integrity_check')
  [ \"\$check\" = ok ] || { echo \"올린 파일 무결성 확인 실패: \$check\" >&2; rm -f .incoming.db; exit 1; }
  mkdir -p backups
  if [ -f cashbook.db ]; then
    sqlite3 cashbook.db \".backup backups/before-restore-$ts.db\"
    echo '기존 DB → /data/backups/before-restore-$ts.db'
  fi
  # 이전 DB의 저널이 남아 있으면 새 DB에 잘못 적용되므로 지운다 (기존 DB는 위 .backup에서 이미 복구·보관됨)
  rm -f cashbook.db-journal cashbook.db-wal cashbook.db-shm
  mv -f .incoming.db cashbook.db
  echo \"복원: \$(sqlite3 cashbook.db 'select count(*) from transactions') 거래\""

trap - EXIT
restore_server
kube rollout status deployment/cashbook --timeout=180s
