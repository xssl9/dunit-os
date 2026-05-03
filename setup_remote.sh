#!/bin/bash

SERVER="root@2.27.40.246"
PASSWORD="vH3mQ2aM4vaDhh"

echo "Setting up Git repository on server..."

sshpass -p "$PASSWORD" ssh "$SERVER" << 'EOF'
cd /root
mkdir -p dunit-os
cd dunit-os
tar -xzf ../dunit-os-source.tar.gz
git init
git config user.name "Dunit OS"
git config user.email "dunit@os.local"
git add .
git commit -m "Initial commit: Dunit OS microkernel"
git config receive.denyCurrentBranch ignore
echo "Git repository created at /root/dunit-os"
EOF

echo ""
echo "Adding remote to local repository..."
git remote remove server 2>/dev/null
git remote add server "root@2.27.40.246:/root/dunit-os"

echo ""
echo "Done! You can now push with:"
echo "git push server main"
