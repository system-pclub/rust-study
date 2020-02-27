#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import sys

INIT = 0
INFO = 1
LOCK = 2
NEXT = 3

def main():
    input_file = sys.argv[1]
    lock_line = ""
    drop_line = ""
    with open(input_file) as infile:
        state = INIT
        for line in infile.readlines():
            if state == INIT:
                if line == "Manual Drop Info:\n":
                    state = INFO
                else:
                    state = NEXT
            elif state == INFO:
                content = line.strip()
                if content.startswith("/rust"):
                    state = NEXT
                else:
                    lock_line = line
                    state = LOCK
            elif state == LOCK:
                content = line.strip()
                if content.startswith("/rust"):
                    state = NEXT
                else:
                    drop_line = line
                    print("Manual Drop Info:")
                    print(lock_line, end="")
                    print(drop_line, end="")
                    state = NEXT
            elif state == NEXT:
                if line == "Manual Drop Info:\n":
                    state = INFO
                else:
                    state = NEXT
            else:
                assert False

if __name__ == "__main__":
    main()