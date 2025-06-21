name = chronyd
description = NTP client daemon
exec = /usr/sbin/chronyd
args = -F 1
restart = on-failure
requires = network-pre
after = wpa_supplicant
