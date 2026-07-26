#!/bin/bash
set -e

GITIGNORE=".gitignore"

cat >> "$GITIGNORE" << 'EOF'

# Rust / Cargo
target/
Cargo.lock

# Soroban / Stellar
contract/target/
.wasm

# Build artifacts
*.log
*.pid

# Environment
.env
.env.local
.env.*.local

# IDE
.vscode/
.idea/
*.swp
*.swo

# OS
.DS_Store
Thumbs.db

# Generated files
issue*.json
hello_chainverse.rs
EOF

echo "Appended cleanup entries to $GITIGNORE"
echo "Review with: git diff $GITIGNORE"
