# syntax=docker/dockerfile:1
# 웹 빌드 결과(dist)만 담은 이미지. CI에서 web-sync Job으로 PVC에 올릴 때 쓴다 (선택).
# 저장소 루트에서 빌드한다:
#   docker buildx build --platform linux/amd64,linux/arm64 -f k8s/docker/web.Dockerfile -t rouddy/cashbook:web-<tag> --push .

FROM node:22-alpine AS build
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci
COPY web/ ./
RUN npm run build

# 복사(cp)·링크 교체(ln, mv -T)만 하면 되므로 busybox
FROM busybox:1.37
COPY --from=build /web/dist /dist
USER 10001:10001
