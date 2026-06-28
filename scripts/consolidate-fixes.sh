#!/bin/bash
set -e

FIXES_DIR="fixes"
AUDIT_DIR="frontend-performance-audit"
CONSOLIDATED_DIR="docs/fixes-archive"

if [ ! -d "$FIXES_DIR" ] && [ ! -d "$AUDIT_DIR" ]; then
  echo "Nothing to consolidate — neither directory exists."
  exit 0
fi

mkdir -p "$CONSOLIDATED_DIR"

if [ -d "$FIXES_DIR" ]; then
  echo "Moving $FIXES_DIR to $CONSOLIDATED_DIR/"
  mv "$FIXES_DIR" "$CONSOLIDATED_DIR/fixes"
fi

if [ -d "$AUDIT_DIR" ]; then
  echo "Moving $AUDIT_DIR to $CONSOLIDATED_DIR/"
  mv "$AUDIT_DIR" "$CONSOLIDATED_DIR/frontend-performance-audit"
fi

echo "Consolidation complete."
echo "Both directories are now under $CONSOLIDATED_DIR/"
echo "After reviewing, run: git rm -r $FIXES_DIR $AUDIT_DIR 2>/dev/null; git add $CONSOLIDATED_DIR"
