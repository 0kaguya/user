if command -v rustc &>/dev/null
    command -v sccache &>/dev/null &&
        set -x RUSTC_WRAPPER sccache
end
