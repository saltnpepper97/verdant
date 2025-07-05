name: tty@{}
desc: Login prompt on {}

cmd: /sbin/getty
args: 38400 {} linux

startup: user

restart: always

instances:
    - tty1
    - tty2
