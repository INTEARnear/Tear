#!/bin/bash

SUBMODULE_PATH="./xeon-private-modules"

declare -a SHIM_PACKAGES=("new-liquidity-pools" "nft-buybot" "potlock" "price-alerts" "socialdb" "new-tokens" "trading-bot")

if [ -f "$SUBMODULE_PATH/.git" ] || [ -d "$SUBMODULE_PATH/.git" ]; then
    echo "Submodule $SUBMODULE_PATH already exists."
    exit 0
fi

git submodule update --init --recursive $SUBMODULE_PATH

if [ -f "$SUBMODULE_PATH/.git" ]; then
    echo "Private modules successfully downloaded."
else
    echo "Failed to download private submodule. Using only open-source modules..."

    mkdir -p $SUBMODULE_PATH

    cd $SUBMODULE_PATH || exit

    for package in "${SHIM_PACKAGES[@]}"; do
        cargo new --lib "$package"
    done

    git init

    cd ..

    git checkout Cargo.toml

    echo "Shim crates installed in $SUBMODULE_PATH, the project is ready to be built using 'cargo build'. You may have seen some warnings like 'failed to load manifest for workspace member $PWD/tearbot', 'failed to load manifest for dependency', 'failed to read $PWD/xeon-private-modules/something/Cargo.toml', you can safely ignore them."
fi
