#!/bin/bash

RYZENADJ="/path/to/ryzenadj"
INTERVAL=5
LASTKNOWN=""

while :
do
	CURPP="`powerprofilesctl get`"

	if [ "$CURPP" != "$LASTKNOWN" ]; then
		LASTKNOWN=$CURPP

		if [ "$CURPP" == "performance" ]; then
			echo "performance"
			sudo $RYZENADJ --stapm-limit=28000 --fast-limit=28000 --slow-limit=28000 --apu-slow-limit=28000
			kscreen-doctor output.1.mode.0
		elif [ "$CURPP" == "power-saver" ]; then
			echo "power-save"
			sudo $RYZENADJ --stapm-limit=15000 --fast-limit=15000 --slow-limit=15000 --apu-slow-limit=15000
			kscreen-doctor output.1.mode.1
		else
			echo "balanced"
			sudo $RYZENADJ --stapm-limit=22000 --fast-limit=22000 --slow-limit=22000 --apu-slow-limit=22000
			kscreen-doctor output.1.mode.1
		fi
	fi

	sleep $INTERVAL
done

