name: Release
on:
  push:
    tags:
      - 'v*.*.*'
env:
  TAG: "$(git rev-parse --short HEAD)"

jobs:
  clean_code_check:
    uses: clean_code.yaml.d

  build_images:
    needs: clean_code_check
    uses: build.yaml.d
    with:
      TAG: ${{ github.ref_name }}-${{ github.sha }}

  run_tests:
    needs: build_images
    uses: ./.github/workflows/test.yaml
