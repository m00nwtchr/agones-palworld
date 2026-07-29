#!/usr/bin/env bash
# build-image.sh — idempotent sidecar image build.
#
# Usage: ./scripts/build-image.sh [image] [tag] [--push]
#   image: target image (default from values.yaml sidecar.image.repository)
#   tag:   target tag (default from Chart.yaml appVersion; falls back to "dev")
#   --push: push to registry after build
set -euo pipefail

IMAGE="${1:-ghcr.io/m00nwtchr/agones-palworld}"
TAG="${2:-$(awk -F'"' '/^appVersion/ {print $2}' helm/Chart.yaml 2>/dev/null || echo dev)}"
PLATFORM_FLAG=""
PUSH=0
for arg in "$@"; do
  case "$arg" in
    --push) PUSH=1 ;;
  esac
done

LABEL_VERSION="$TAG"
LABEL_KEY="org.opencontainers.image.version=$LABEL_VERSION"

CMD=(docker buildx build --load --tag "$IMAGE:$TAG" --label "$LABEL_KEY" .)
if [[ "$PUSH" -eq 1 ]]; then
  CMD=(docker buildx build --push --tag "$IMAGE:$TAG" --label "$LABEL_KEY" .)
fi

echo "+ ${CMD[*]}"
"${CMD[@]}"
