#!/usr/bin/bash

ROTATION="`kscreen-doctor -o | grep Rotation | cut -d ' ' -f 2`"

if [[ "$ROTATION" == *"1"* ]]; then
	kscreen-doctor output.1.rotation.left
else
	kscreen-doctor output.1.rotation.normal
fi

