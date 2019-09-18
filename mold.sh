#!/bin/sh
# This script will download the mold executable from $URL as $EXE, then pass
# all arguments through to it. This simplifies usage on CI/CD platforms as well
# as for users who haven't installed mold yet.

VER="0.3.2"
EXE="./.mold-$VER"
URL="https://github.com/xtfc/mold/releases/download/v$VER/mold-$VER"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

if [ ! -f $EXE ]; then
  # decide whether we can use curl or wget
  if hash curl 2>/dev/null; then
    CMD="curl -sSfL $URL -o $EXE"
  elif hash wget 2>/dev/null; then
    CMD="wget -q $URL -O $EXE"
  else
    printf $RED"Error:"$NC" could not find curl or wget\n"
    exit 1
  fi

  # download or exit
  printf $GREEN" Downloading"$NC" mold v$VER\r"
  if $CMD; then
    chmod +x $EXE
    printf $GREEN"  Downloaded"$NC" mold v$VER\n"
  else
    printf $RED"Error:"$NC" could not download mold v$VER\n"
    rm $EXE
    exit 1
  fi
fi

$EXE $@
