set -x ANTHROPIC_BASE_URL "https://openrouter.ai/api"
set -x ANTHROPIC_AUTH_TOKEN $OPENROUTER_API_KEY
set -x ANTHROPIC_API_KEY ""

# Make "CLAUDE_MODEL" an alias of "ANTHROPIC_DEFAULT_SONNET_MODEL", so
# the default model can later be customized by `set -Ux CLAUDE_MODEL ...`.
function set-anthropic-default-sonnect-model -v CLAUDE_MODEL
    test -n "$CLAUDE_MODEL" &&
        set -Ux ANTHROPIC_DEFAULT_SONNET_MODEL $CLAUDE_MODEL
end
# Not sure if --on-variable (-v) works with uninitialized variable so here we
# initialize it with empty string.
set -q CLAUDE_MODEL || set -Ux CLAUDE_MODEL ""
