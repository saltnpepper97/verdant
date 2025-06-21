name = wpa_supplicant
description = WPA Supplicant for managing Wi-Fi
exec = /sbin/wpa_supplicant
args = -B -i wlan0 -c /etc/wpa_supplicant/wpa_supplicant.conf
restart = on-failure
requires = network-pre
