#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import sys
import os

def remove_comments(lines):

    unsafe_block = 0
    unsafe_fn = 0
    unsafe_trait = 0

    in_unsafe = False
    num_lcb_in_unsafe = 0

    for line_idx, line in enumerate(lines):
        # remove comment
        pos = line.find("//")
        if pos != -1:
            old_line = line
            line = line[:pos] + '\n'

        if in_unsafe:
            for idx, char in enumerate(line):
                if char == '{':
                    num_lcb_in_unsafe += 1
                elif char == '}':
                    num_lcb_in_unsafe -= 1
                    if num_lcb_in_unsafe == 0:
                        in_unsafe = False
                        print(str(line_idx) + ': ' + line[:idx+1])
                        break
            if in_unsafe:
                print(str(line_idx) + ': ' + line, end='')

        else:
            # find unsafe keyword
            pos_unsafe = line.find("unsafe ")
            has_printed = False
            # if unsafe found
            if pos_unsafe != -1:
                pos_after_unsafe = pos_unsafe + len("unsafe ")
                pos_lcb = line[pos_after_unsafe:].find("{")

                # unsafe .*[fn|impl|trait]
                if "fn " in line[pos_after_unsafe:]:
                    unsafe_fn += 1

                if "trait " in line[pos_after_unsafe:]:
                    unsafe_trait += 1

                if "fn" in line[pos_after_unsafe:] or "impl" in line[pos_after_unsafe:] or "trait" in line[pos_after_unsafe:]:
                    print('\n')
                    print(str(line_idx) + ': ' + line, end='')
                    has_printed = True

                end_pos = -1
                # if { found
                if pos_lcb != -1:
                    unsafe_block += 1
                    in_unsafe = True
                    num_lcb_in_unsafe = 0
                    for idx, char in enumerate(line[pos_lcb+1:]):
                        if char == '{':
                            num_lcb_in_unsafe += 1
                        elif char == '}':
                            num_lcb_in_unsafe -= 1
                            if num_lcb_in_unsafe == 0:
                                end_pos = pos_lcb+1+idx+1
                                in_unsafe = False

                if not has_printed:
                    if end_pos == -1:
                        print('\n')
                        print(str(line_idx) + ': ' + line[pos_lcb:], end='')
                    else:
                        print(str(line_idx) + ': ' + line[pos_lcb:end_pos])

    return unsafe_block, unsafe_fn, unsafe_trait                                

def parse_file(infile):
    return remove_comments(infile.readlines())


def main():
    sum_unsafe_block = 0
    sum_unsafe_fn = 0
    sum_unsafe_trait = 0

    for root, dirs, files in os.walk(sys.argv[1], topdown=False):
        for name in files:
            if not name.endswith(".rs"):
                continue
            filepath = os.path.join(root, name)
            print(filepath)
            with open(filepath) as infile:
                unsafe_block, unsafe_fn, unsafe_trait = parse_file(infile)
                sum_unsafe_block += unsafe_block
                sum_unsafe_fn += unsafe_fn
                sum_unsafe_trait += unsafe_trait
    print(sum_unsafe_block, sum_unsafe_fn, sum_unsafe_trait)


if __name__ == "__main__":
    main()
