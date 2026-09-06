#!/usr/bin/env sh
# duflow Claude Code hook'u. Tüm mantık `duflow hook <event>` içinde; binary yoksa ya da
# cwd'de flows/ yoksa sessizce geçer, plugin global kurulunca alakasız repoları rahatsız etmez.
event="$1"
if ! command -v duflow >/dev/null 2>&1; then
  if [ "$event" = "session-start" ] && [ -d flows ]; then
    echo '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"This repo has a flows/ directory but the duflow CLI is not on PATH. Install: curl -fsSL https://raw.githubusercontent.com/duhanbalci/duflow/main/install.sh | sh"}}'
  fi
  exit 0
fi
# Hatalar (eski binary'de `hook` komutu yok, git dışı dizin...) hiçbir zaman engelleyici olmasın:
# exit 2 PreToolUse/Stop'u bloklar. Sessiz geç.
duflow hook "$event" 2>/dev/null || exit 0
