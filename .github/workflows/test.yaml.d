name: Test
on:
  workflow_call:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
    - uses: actions/checkout@v3
      with:
        submodules: 'recursive'

    - name: Setup Rust
      uses: ./.github/actions/setup-rust
      with:
        caller-workflow-name: test

    - name: Run tests
      run: RUST_LOG=info cargo test
