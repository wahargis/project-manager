#!/bin/bash
# project-manager hook commands for claude-code integration
# These are called by claude-code hooks, not by the user directly.
# They extend the base project-manager with lifecycle events.
# Supports any project via RESEARCH_PROJECT env var.

DEFAULT_PROJECT="volta-renaissance"
PROJECT_NAME="${RESEARCH_PROJECT:-$DEFAULT_PROJECT}"

# Find project directory (same logic as project-manager)
if [ -d "/home/atari2036/gen-ai/ik_llama.cpp/docs/$PROJECT_NAME" ]; then
    PROJECT_DIR="/home/atari2036/gen-ai/ik_llama.cpp/docs/$PROJECT_NAME"
elif [ -d "/home/atari2036/projects/home_cloud/docs/$PROJECT_NAME" ]; then
    PROJECT_DIR="/home/atari2036/projects/home_cloud/docs/$PROJECT_NAME"
else
    PROJECT_DIR="/home/atari2036/gen-ai/ik_llama.cpp/docs/$PROJECT_NAME"
fi

JOURNAL="$PROJECT_DIR/research-journal.md"

case "${1:-}" in
    load-context)
        # SessionStart hook: inject project context into conversation
        # Must output JSON with hookSpecificOutput.additionalContext
        CONTEXT=""

        # Last 3 journal entries
        if [ -f "$JOURNAL" ]; then
            RECENT=$(grep -A1 '###' "$JOURNAL" | tail -9 | sed 's/"/\\"/g' | tr '\n' ' ')
            CONTEXT="$CONTEXT## Recent Research Journal\n$RECENT\n\n"
        fi

        # Open items
        if [ -f "$JOURNAL" ]; then
            TODOS=$(grep -iE 'TODO|NEED|BLOCKED|PENDING|REVERTED|FAIL' "$JOURNAL" | grep -v '###' | tail -5 | sed 's/"/\\"/g' | tr '\n' ' ')
            if [ -n "$TODOS" ]; then
                CONTEXT="$CONTEXT## Open Items\n$TODOS\n\n"
            fi
        fi

        # Doc inventory
        DOCS=$(ls -1 "$PROJECT_DIR"/*.md 2>/dev/null | while read f; do echo "$(basename "$f") ($(wc -l < "$f")L)"; done | tr '\n' ', ')
        CONTEXT="$CONTEXT## Project Docs: $DOCS\n"

        # Output JSON for claude-code SessionStart hook
        printf '{"continue":true,"suppressOutput":true,"hookSpecificOutput":{"additionalContext":"# %s Project Context\\n\\n%s"}}' "$PROJECT_NAME" "$CONTEXT"
        ;;

    checkpoint-state)
        # Stop hook: auto-journal a checkpoint
        TS=$(date '+%Y-%m-%d %H:%M')
        echo "### [$TS] [AUTO-CHECKPOINT]" >> "$JOURNAL"
        echo "Session checkpoint. Context may be compacting." >> "$JOURNAL"
        echo "" >> "$JOURNAL"
        ;;

    auto-commit)
        # SessionEnd hook: auto-commit research logs
        GITROOT=$(cd "$PROJECT_DIR" && git rev-parse --show-toplevel 2>/dev/null)
        if [ -n "$GITROOT" ]; then
            cd "$GITROOT"
            git add "$PROJECT_DIR"/*.md 2>/dev/null
            if ! git diff --cached --quiet 2>/dev/null; then
                git commit -m "docs(volta-renaissance): auto-save research logs (session end)

Co-Authored-By: Claude Opus 4.6 (1M context) <noreply@anthropic.com>" 2>/dev/null
            fi
        fi
        ;;
esac
