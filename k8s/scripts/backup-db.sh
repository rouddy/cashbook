#!/usr/bin/env bash
# 실행 중인 서버 Pod에서 DB의 한 시점 사본을 SQLite 파일로 받는다. 서버를 멈추지 않는다.
# - SQLite: sqlite3 온라인 백업(.backup)
# - MySQL: cashbook copy-db로 한 트랜잭션 스냅샷을 SQLite 파일에 복사 (copy-db로 어느 DB로든 되돌릴 수 있음)
#
#   k8s/scripts/backup-db.sh                       # ./cashbook-backup-<시각>.db
#   k8s/scripts/backup-db.sh ~/backups/cash.db
set -euo pipefail
source "$(dirname "$0")/_lib.sh"

kind="$(db_kind)"
[[ "$kind" == sqlite || "$kind" == mysql ]] || die "Secret cashbook-secret의 DATABASE_URL을 확인하세요 ($kind)"
out="${1:-cashbook-backup-$(date +%Y%m%d-%H%M%S).db}"
[[ -e "$out" ]] && die "$out이 이미 있습니다"
pod="$(server_pod)"
[[ -n "$pod" ]] || die "실행 중인 서버 Pod가 없습니다"

tmp="/tmp/cashbook-backup-$$.db"
trap 'kube exec "$pod" -- rm -f "$tmp" >/dev/null 2>&1 || true' EXIT

if [[ "$kind" == sqlite ]]; then
  kube exec "$pod" -- sqlite3 /data/cashbook.db ".backup '$tmp'"
else
  # DATABASE_URL은 Pod 환경 변수에서 읽는다 (명령줄·로그에 비밀번호를 남기지 않음)
  kube exec "$pod" -- sh -c "cashbook copy-db \"\$DATABASE_URL\" 'sqlite://$tmp?mode=rwc' >/dev/null"
fi
check="$(kube exec "$pod" -- sqlite3 "$tmp" "PRAGMA integrity_check")"
[[ "$check" == ok ]] || die "백업 무결성 확인 실패: $check"
kube cp "$pod:$tmp" "$out" >/dev/null
echo "백업($kind): $out ($(wc -c <"$out" | tr -d ' ') bytes, $pod)"
