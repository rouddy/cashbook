# syntax=docker/dockerfile:1
# Rocket 서버 이미지. 저장소 루트에서 빌드한다:
#   k8s/docker/build_and_push.sh   (Docker Hub rouddy/cashbook에 amd64+arm64로 푸시, README 1장)
# 빌드 컨텍스트는 server.Dockerfile.dockerignore(허용 목록)로 server/ 소스만 들어간다 (DB·import 데이터 제외).

FROM rust:1-trixie AS build
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked && cp target/release/cashbook /cashbook

FROM debian:trixie-slim
# sqlite3: 실행 중 백업(.backup)·무결성 확인용. tar(기본 포함): kubectl cp용
RUN apt-get update \
    && apt-get install -y --no-install-recommends sqlite3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 cashbook \
    && useradd --uid 10001 --gid 10001 --home-dir /app --no-create-home --shell /usr/sbin/nologin cashbook
WORKDIR /app
# 요청 크기 제한 등 Rocket 설정 (address/port는 아래 환경 변수가 우선)
COPY server/Rocket.toml ./Rocket.toml
COPY --from=build /cashbook /usr/local/bin/cashbook

# 기본 경로: /data = SQLite PVC, /web/current = 웹 빌드 PVC의 현재 릴리스 링크
ENV DATABASE_URL="sqlite:///data/cashbook.db?mode=rwc" \
    CASHBOOK_WEB_DIR=/web/current \
    CASHBOOK_SECRET_FILE=/data/secret.key \
    ROCKET_ADDRESS=0.0.0.0 \
    ROCKET_PORT=8000

USER 10001:10001
EXPOSE 8000
ENTRYPOINT ["cashbook"]
CMD ["serve"]
