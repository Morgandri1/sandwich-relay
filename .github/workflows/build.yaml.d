name: Build
on:
  workflow_call:
    inputs:
      TAG:
        required: true
        type: string

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
        with:
          submodules: 'recursive'

      - uses: jpribyl/action-docker-layer-caching@v0.1.1
        # Ignore the failure of a step and avoid terminating the job.
        continue-on-error: true

      - name: Build containers
        run: docker compose build --progress=plain
        env:
          COMPOSE_DOCKER_CLI_BUILD: 1
          DOCKER_BUILDKIT: 1
          ORG: jitolabs
          TAG: ${{ inputs.TAG }}
