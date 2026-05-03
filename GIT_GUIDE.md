# Git Repository Guide - Dunit OS

## Repository Location

**Server**: `root@2.27.40.246:/root/dunit-os`  
**Password**: `vH3mQ2aM4vaDhh`

## Initial Setup (First Time)

```bash
# Clone repository
git clone root@2.27.40.246:/root/dunit-os
cd dunit-os

# Configure your identity
git config user.name "Your Name"
git config user.email "your@email.com"
```

## Daily Workflow

### Before Starting Work
```bash
# Always pull latest changes first
git pull
```

### During Work
```bash
# Check what changed
git status

# Add changes
git add .

# Commit with message
git commit -m "Description of changes"

# Push to server
git push
```

### If Push Conflicts Occur
```bash
# Pull changes first
git pull

# If conflicts exist, edit files manually
# Then add resolved files
git add .

# Complete merge
git commit -m "Resolved conflicts"

# Push again
git push
```

## Useful Commands

```bash
# View commit history
git log --oneline

# View who changed what in a file
git blame filename

# Discard changes in file (before commit)
git checkout -- filename

# View differences
git diff

# View current branch
git branch

# Create new branch
git checkout -b feature-name

# Switch branch
git checkout main
```

## Team Rules

1. **Always `git pull` before starting work**
2. **Commit often** - small commits are better
3. **Write clear commit messages**
4. **Pull before push** - avoid conflicts
5. **Don't commit build artifacts** - check `.gitignore`

## Quick Reference

```bash
git pull                              # Get latest changes
# ... work on code ...
git add .                             # Stage all changes
git commit -m "Added feature X"       # Commit
git pull                              # Pull again to be safe
git push                              # Push to server
```

## SSH Password

You'll need to enter password for each `pull`/`push`/`clone`:
```
vH3mQ2aM4vaDhh
```

## Setting Up SSH Key (Optional - No Password)

```bash
# Generate SSH key
ssh-keygen -t ed25519 -C "your@email.com"

# Copy public key to server
ssh-copy-id root@2.27.40.246

# Test connection
ssh root@2.27.40.246
```

After this, you won't need password for git operations.

## Repository Structure

```
dunit-os/
├── hal/              # Hardware Abstraction Layer (C/ASM)
├── kernel/           # Kernel (Rust)
├── userspace/        # Userspace programs
├── limine/           # Bootloader
├── Makefile          # Build system
├── BUILD.md          # Build instructions
└── GIT_GUIDE.md      # This file
```

## Building the OS

```bash
make clean
make
./create_iso.sh
qemu-system-x86_64 -cdrom build/microkernel.iso -m 512M
```

See `BUILD.md` for detailed build instructions.

## Troubleshooting

**Problem**: `Permission denied (publickey,password)`  
**Solution**: Check password is correct: `vH3mQ2aM4vaDhh`

**Problem**: `fatal: refusing to merge unrelated histories`  
**Solution**: `git pull --allow-unrelated-histories`

**Problem**: Merge conflicts  
**Solution**: Edit conflicted files, remove `<<<<<<<`, `=======`, `>>>>>>>` markers, then `git add` and `git commit`

## Contact

For questions about the repository setup, contact the team lead.
