#!/usr/bin/bash

if [ "$(id -u)" -ne 0 ]; then
    echo "This script must be run as root, use sudo" >&2
    exit 1
fi

echo "installing sound fix"

# make dir if it doesn't exist
mkdir -p /etc/libinput

cat <<EOF > "/etc/libinput/local-overrides.quirks"
[Minisforum V3 volume keys]
MatchName=AT Translated Set 2 keyboard
MatchDMIModalias=dmi:*svnMicroComputer(HK)TechLimited:pnV3:*
ModelTabletModeNoSuspend=1
EOF

echo "done, reboot to see changes"
echo "if you wish to undo changes, delete the /etc/libinput/local-overrides.quirks file"