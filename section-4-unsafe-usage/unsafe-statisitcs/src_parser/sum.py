#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import sys

def main():
    input_file_path =sys.argv[1]
    sum_LOC = 0
    with open(input_file_path) as infile:
        lines = infile.readlines()
        for line in lines:
            fields = line.split(",")
            assert len(fields) == 2
            sum_LOC += len(fields[1]) - len(fields[0]) + 1
    print(len(lines), sum_LOC)

if __name__ == "__main__":
    main()
