name: acpid
desc: ACPI daemon for power management events

cmd: /usr/sbin/acpid
args: -f

startup-package: base

restart: on-failure
restart-delay: 3
