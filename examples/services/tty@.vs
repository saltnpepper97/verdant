name: tty@{}
desc: Login terminal on {}

cmd: /sbin/getty
args: "115200 {} linux"

stdout: /dev/{}
stderr: /dev/{}

instances:
    - tty1
    - tty2
