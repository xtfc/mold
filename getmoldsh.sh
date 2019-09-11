#!/bin/sh
# This script will download the mold.sh executable from $URL as $EXE, then exit

REF="master"
EXE="mold.sh"
URL="https://raw.githubusercontent.com/xtfc/mold/$REF/mold.sh"

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

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
printf $GREEN" Downloading"$NC" mold.sh $BLUE$REF$NC\r"
if $CMD; then
  chmod +x $EXE
  printf $GREEN"  Downloaded"$NC" mold.sh $BLUE$REF$NC\n"
else
  printf $RED"Error:"$NC" could not download mold.sh $BLUE$REF$NC\n"
  rm $EXE
  exit 1
fi
