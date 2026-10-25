#!/usr/bin/env bash
# Human-in-the-loop reproduction loop for desktop / window / tray interactions.
# Copy this file to the scratch directory, edit the steps below, and run it.
# The agent runs the script; the user follows prompts in their terminal.
#
# Usage:
#   bash hitl-loop.template.sh
#
# Two helpers:
#   step "<instruction>"          -> show instruction, wait for Enter
#   capture VAR "<question>"      -> show question, read response into VAR
#
# At the end, captured values are printed as KEY=VALUE for the agent to parse.

set -euo pipefail

step() {
  printf '\n>>> %s\n' "$1"
  read -r -p "    [Enter when done] " _
}

capture() {
  local var="$1" question="$2" answer
  printf '\n>>> %s\n' "$question"
  read -r -p "    > " answer
  printf -v "$var" '%s' "$answer"
}

# --- edit below ---------------------------------------------------------

step "Launch Catffeine ('npm run tauri dev') and right-click the companion cat."

capture MENU_VISIBLE "Did the context menu open and stay interactive? (y/n)"

capture CAT_STATE "What state label is displayed on hover? (Sleeping / Staying Awake / Working)"

# --- edit above ---------------------------------------------------------

printf '\n--- Captured ---\n'
printf 'MENU_VISIBLE=%s\n' "$MENU_VISIBLE"
printf 'CAT_STATE=%s\n' "$CAT_STATE"
