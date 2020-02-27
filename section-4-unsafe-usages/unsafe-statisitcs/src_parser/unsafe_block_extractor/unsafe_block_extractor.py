#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import sys

class UnsafeBlockInfo:
    def __init__(self):
        self.start_line_no = 0 
        self.end_line_no = 0
    def __str__(self):
        return str(self.start_line_no) + "," + str(self.end_line_no)
    def __repr__(self):
        return str(self.start_line_no) + "," + str(self.end_line_no)

class UnsafeFnInfo:
    def __init__(self):
        self.start_line_no = 0 
        self.end_line_no = 0
    def __str__(self):
        return str(self.start_line_no) + "," + str(self.end_line_no)
    def __repr__(self):
        return str(self.start_line_no) + "," + str(self.end_line_no)

def extract_macro(lines):
    pattern = "unsafe {"
    pattern_len = len(pattern)
    is_in_unsafe_block = False
    unsafe_block_infos = []
    cur_unsafe_block_info = UnsafeBlockInfo()
    left = 0
    for idx, line in enumerate(lines):
        if not is_in_unsafe_block:
            pos = line.find(pattern)
            if pos != -1:
                remain = line[pos+pattern_len:]
                is_in_unsafe_block = True
                cur_unsafe_block_info.start_line_no = idx + 1
                left = 1
                for ch in remain:
                    if ch == '{':
                        left += 1
                    elif ch == '}':
                        left -= 1
                        if left == 0:
                            is_in_unsafe_block = False
                            cur_unsafe_block_info.end_line_no = idx + 1
                            unsafe_block_infos.append(cur_unsafe_block_info)
                            cur_unsafe_block_info = UnsafeBlockInfo()
        else: # is_in_unsafe_block
            for ch in line:
                if ch == '{':
                    left += 1
                elif ch == '}':
                    left -= 1
                    if left == 0:
                        is_in_unsafe_block = False
                        cur_unsafe_block_info.end_line_no = idx + 1
                        unsafe_block_infos.append(cur_unsafe_block_info)
                        cur_unsafe_block_info = UnsafeBlockInfo()
            
    return unsafe_block_infos

def main():
    with open(sys.argv[1]) as infile:
        lines = infile.readlines()
        unsafe_block_infos = extract_macro(lines)
        for unsafe_block_info in unsafe_block_infos:
            print(unsafe_block_info)

if __name__ == "__main__":
    main()
