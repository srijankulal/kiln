#!/usr/bin/env bash
# Helper script to create and push a release tag matching the 0.xxx.xx convention.
set -euo pipefail

TAG="${1:-}"

if [[ -z "$TAG" ]]; then
    echo "Usage: ./scripts/create-release.sh <0.xxx.xx>"
    echo "Example: ./scripts/create-release.sh 0.002.00"
    exit 1
fi

if [[ ! "$TAG" =~ ^v?0\.[0-9]{1,3}\.[0-9]{1,2}$ ]]; then
    echo "Error: Tag '$TAG' does not match the 0.xxx.xx convention (e.g., 0.002.00)."
    exit 1
fi

echo "Creating release tag '$TAG'..."
git tag -a "$TAG" -m "Release $TAG"
echo "Pushing tag '$TAG' to origin..."
git push origin "$TAG"
echo "Release tag '$TAG' pushed successfully! GitHub Actions will now build and publish the release."
