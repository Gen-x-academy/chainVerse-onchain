#!/bin/bash
set -e

echo "=== Orphan File Cleanup ==="

# Remove root-level hello_chainverse files
for f in hello_chainverse.rs hello_chainverse_README.md; do
  if [ -f "$f" ]; then
    echo "Removing $f"
    rm "$f"
    git rm --cached "$f" 2>/dev/null || true
  fi
done

# Remove orphan JSON files at root
for f in issue*.json; do
  if [ -f "$f" ]; then
    echo "Removing $f"
    rm "$f"
    git rm --cached "$f" 2>/dev/null || true
  fi
done

# Remove issue JSON files in subdirectories
find . -maxdepth 2 -name "issue*.json" -type f | while read -r f; do
  echo "Removing $f"
  rm "$f"
  git rm --cached "$f" 2>/dev/null || true
done

echo ""
echo "=== Cleanup Summary ==="
echo "Orphan files have been removed."
echo "Run 'git add -A && git commit -m \"chore: remove orphan files\"' to commit changes."
