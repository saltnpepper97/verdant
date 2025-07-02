name: tty@{id}
desc: Login prompt using getty on /dev/{id}

cmd: /sbin/getty
args: -L 115200 /dev/{id} linux

restart: always

tags: tty

instances:
    - tty1
    - tty2
    - tty3
    - tty4
    - tty5
    - tty6
