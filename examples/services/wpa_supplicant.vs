name: wpa_supplicant
desc: WiFi connection manager

cmd: /sbin/wpa_supplicant
args: -c /etc/wpa_supplicant/wpa_supplicant.conf -i wlan0 -B

restart: always
restart-delay: 3

priority: 40

tags: net, wifi
