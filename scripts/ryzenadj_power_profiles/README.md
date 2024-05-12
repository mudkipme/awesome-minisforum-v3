# Ryzenadj Power Profiles

This will allow you to adjust the Minisforum V3's TDP and refresh rate by
adjusting the Power Profile from within the KDE plasma desktop.

By default, the settings are:

* Power-Saver: 15w TDP, 60hz refresh rate
* Balanced: 22w TDP, 60hz refresh rate
* Performance: 28w TDP, 165hz refresh rate

## Initial setup

### Ryzenadj
Make sure to setup and install ryzenadj. You may need to disable secure boot
to get it working. The kernel on Bazzite is configured such that it works with
secure boot on.

### Set ryzenadj to run as sudo without a password
Run the command:

```
sudo visudo
```

Then add the following line at the bottom of the file (changing the path to be where your ryzenadj binary is located).

```
ALL ALL=NOPASSWD: /path/to/ryzenadj
```

### power_adjust.sh
Copy power_adjust.sh to the location you would like to store it. Then, edit the file to set the path to the ryzenadj binary. You can also change the TDP settings and screen refresh settings to your liking.

### v3_power_profiles.service
Copy v3_power_profiles.service to $HOME/.config/systemd/user/v3_power_profiles.service.

Next run:

```
systemctl --user enable v3_power_profiles.service
```

Then,

```
systemctl --user start v3_power_profiles.service
```

## Conclusion
You should now be able to adjust the power profiles in KDE and see the TDP and
screen resolution adjust after a few seconds.
