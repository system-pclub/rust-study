#!/usr/bin/env python3
# -*- coding: utf-8 -*-

import sys

class UnsafeFnInfo:
    def __init__(self):
        self.start_line_no = 0 
        self.end_line_no = 0
    def __str__(self):
        return str(self.start_line_no) + "," + str(self.end_line_no)
    def __repr__(self):
        return str(self.start_line_no) + "," + str(self.end_line_no)

def locate_unsafe_fn(line):
    unsafe_pattern = "unsafe "
    unsafe_pattern_len = len(unsafe_pattern)
    fn_pattern = "fn "
    fn_pattern_len = len(fn_pattern)

    pos = line.find(unsafe_pattern)
    if pos != -1:
        remain = line[pos+unsafe_pattern_len:]
        pos2 = remain.find(fn_pattern)
        if pos2 != -1:
            remain2 = remain[pos2+fn_pattern_len:]
            pos3 = remain2.find("{")
            if pos3 != -1:
                return pos+unsafe_pattern_len+pos2+fn_pattern_len+pos3+1
    return -1
    
def extract_unsafe_fn(lines):
    is_in_unsafe_fn = False
    unsafe_fn_infos = []
    cur_unsafe_fn_info = UnsafeFnInfo()
    left = 0
    for idx, line in enumerate(lines):
        if not is_in_unsafe_fn:
            pos = locate_unsafe_fn(line)
            if pos != -1:
                remain = line[pos:]
                is_in_unsafe_fn = True
                cur_unsafe_fn_info.start_line_no = idx + 1
                left = 1
                for ch in remain:
                    if ch == '{':
                        left += 1
                    elif ch == '}':
                        left -= 1
                        if left == 0:
                            is_in_unsafe_fn = False
                            cur_unsafe_fn_info.end_line_no = idx + 1
                            unsafe_fn_infos.append(cur_unsafe_fn_info)
                            cur_unsafe_fn_info = UnsafeFnInfo()
        else: # is_in_unsafe_fn
            for ch in line:
                if ch == '{':
                    left += 1
                elif ch == '}':
                    left -= 1
                    if left == 0:
                        is_in_unsafe_fn = False
                        cur_unsafe_fn_info.end_line_no = idx + 1
                        unsafe_fn_infos.append(cur_unsafe_fn_info)
                        cur_unsafe_fn_info = UnsafeFnInfo()
            
    return unsafe_fn_infos

def main():
    with open(sys.argv[1]) as infile:
        lines = infile.readlines()
        unsafe_fn_infos = extract_unsafe_fn(lines)
        for unsafe_fn_info in unsafe_fn_infos:
            print(unsafe_fn_info)

if __name__ == "__main__":
    main()
