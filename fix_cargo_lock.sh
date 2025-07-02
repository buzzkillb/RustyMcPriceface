#!/bin/bash

# Backup the original Cargo.lock
cp Cargo.lock Cargo.lock.backup

# Replace version 4 with version 3
sed -i 's/version = 4/version = 3/' Cargo.lock

echo "Cargo.lock downgraded to version 3 for compatibility"
echo "Original file backed up as Cargo.lock.backup" 