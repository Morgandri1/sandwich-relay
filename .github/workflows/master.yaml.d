name: Master
on:
  push:
    branches:
      - master

jobs:
  clean_code_check:
    uses: ./.github/workflows/clean_code.yaml

  build_images:
    needs: clean_code_check
    uses: ./.github/workflows/build.yaml
    with:
      TAG: "latest"

  run_tests:
    needs: build_images
    uses: ./.github/workflows/test.yaml
