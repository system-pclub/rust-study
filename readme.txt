The dataset, analysis scripts and bug detectors for PLDI 2020 Artifact Evaluation
---------------------------------------------------------------------------------

*************************************************************************************************
*Version: 1.0
*Update:  Feb 28, 2020
*Paper:   Understanding Memory and Thread Safety Practices and Issues in Real-World Rust Programs
*************************************************************************************************

This document is to help users make use of the dataset we collected and 
reproduce the numbers we reported in our submission. It contains the 
following descriptions:



0. Artifact Expectation
-----------------------------------

The information of our collected data is released in an excel. The scripts 
and the bug detectors are released in a virtual machine, which is created 
using Virtual Box 5.1.38. We expect a Virtual Box in the version or higher 
to open the release virtual machine.



1. Artifact Overview
-----------------------------------

Our paper presents an empirical study of safety practices and safety issues 
in Rust. For artifact evaluation, we release 1) the study results of sampled 
unsafe usages, 2) the study results of collected memory and concurrency bugs, 
3) the scripts to compute the numbers and plot the figures in our paper, 
and 4) the built bug detectors.

In total, we sampled 850 unsafe code usages, 70 memory bugs and 100 concurrency 
bugs from five open-source Rust projects, five widely-used Rust libraries, 
two online security databases, and the Rust standard library. Where we collected 
these study objects and the detailed labels of them are released using a google excel 
"artifact.xlsx" (https://docs.google.com/spreadsheets/d/1GWbiPSBCIbqH2g3Vzc-o2XdxOskXyDhkJjNUjFi1pzs/edit?usp=sharing). 
All columns and tabs discussed later are inside the excel, unless otherwise 
specified. 

Our analysis scripts and built bug detectors are already open-sourced on 
GitHub (https://github.com/system-pclub/rust-study.git). To facilitate the 
production of our results, we also release a virtual machine. In the virtual 
machine, we have already installed all the required libraries, checked out 
our code, and downloaded the analyzed data. The user name of the virtual 
machine is "user" and the password is "123". 



2. Background and Related Work (Section 2)
---------------------------------------------------------

Figure 1. The figure can be plotted by executing the following commands 
on the virtual machine: 
```
cd ~/pldi-2020/rust-study/section-2-background-and-related-work/Figure-1
./plot_Figure_1.sh
```

The raw data is available at 
https://github.com/system-pclub/rust-study/blob/master/section-2-background-and-related-work/Figure-1/data_rust_history.tab

The detailed explanation of the raw data format is available at 
https://github.com/system-pclub/rust-study/tree/master/section-2-background-and-related-work/Figure-1


Figure 2. The figure can be plotted by executing the following commands 
on the virtual machine: 
```
cd ~/pldi-2020/rust-study/section-2-background-and-related-work/Figure-2
./plot_Figure_2.sh
```

The raw data is available at 
https://github.com/system-pclub/rust-study/tree/master/section-2-background-and-related-work/Figure-2/raw_data

The detailed explanation of the raw data format is available at 
https://github.com/system-pclub/rust-study/tree/master/section-2-background-and-related-work/Figure-2

Lines 196-197. "Among the 170 bugs, 145 of them were fixed after 2016."
The raw data about when each collected bug was fixed is released in column "D" 
of tab "section-5-memory", column "E" of tab "section-6.1-blocking", 
and column "E" of tab "section-6.2-non-blocking". 



3. Application and Methodology (Section 3)
---------------------------------------------------------

Table 1. We combine the information of the five libraries in one row. 
The detailed information of the five libraries is in tab "section-3". 

Line 418. "In total, we studied 70 memory and 100 concurrency bugs." 
The commit numbers and the CVE numbers of our collected bugs are released 
in column "B" of tab "section-5-memory", column "B" of tab 
"section-6.1-blocking", and column "B" of tab "section-6.2-non-blocking".



4. Unsafe Usages (Section 4)
---------------------------------------------------------

Lines 428 - 434. "We found 12835 unsafe usages in our studied applications in 
Table 1, including 7061 unsafe code regions, 5727 unsafe functions, 
and 47 unsafe traits. In Rust’s standard library (Rust std for short), 
we found 1577 unsafe code regions, 870 unsafe functions, and 12 unsafe traits." 
The detailed numbers are in tab "section-4-stat". They can be generated 
by executing the following commands on the virtual machine: 
```
cd ~/pldi-2020/rust-study/section-4-unsafe-usages/unsafe-statisitcs/src_parser
./run_all.sh
```

Lines 436 - 441. "We randomly select 600 unsafe usages from our studied applications, 
including 400 interior unsafe usages and 200 unsafe functions." The sampled unsafe usages 
are in tab "section-4.1-usage".

Lines 441 - 442. "We also studied 250 interior unsafe usages in Rust std". 
The sampled interior unsafe usages from Rust std are in tab "section-4.3-interior". 
The commit number for the studied Rust version is 2975a3c4befa8ad610da2e3c5f5de351d6d70a2b.

Lines 451 - 453. "Most of them (66%) are for (unsafe) memory operations, such as 
raw pointer manipulation and type casting. Calling unsafe functions counts for 29% 
of the total unsafe usages." The detailed numbers are in column "F", column "P" 
and column "X" of tab "section-4.1-usage".

Lines 459 - 461. "To understand the reasons why programmers use unsafe code, 
we further analyze the purposes of our studied 600 unsafe usages." The detailed 
numbers are in columns "Z" - "AE" of tab "section-4.1-usage".

Lines 468 - 471. "Our experiments show that unsafe memory copy with 
ptr::copy_nonoverlapping() is 23% faster than the slice::copy_from_slice() in some case." 
The performance number can be got by executing the following command on 
the virtual machine: 
```
cd ~/pldi-2020/rust-study/section-4-unsafe-usages/section-4-1-reasons-of-usage/mem-copy
cargo bench
```

Lines 471 - 473. "Unsafe memory access with slice::get_ unchecked() is 4-5× 
faster than the safe memory access with boundary checking."  The performance 
number can be got by executing the following command on the virtual machine. 
```
cd ~/pldi-2020/rust-study/section-4-unsafe-usages/section-4-1-reasons-of-usage/array-access
cargo bench
```

Lines 473 - 475. "Traversing an array by pointer computing (ptr::offset()) 
and dereferencing is also 4-5× faster than the safe array access with boundary checking." 
The performance number can be got by executing the following command on 
the virtual machine:
```
cd ~/pldi-2020/rust-study/section-4-unsafe-usages/section-4-1-reasons-of-usage/array-offset
cargo bench
```

Lines 479 - 483. "One interesting finding is that programmers sometimes mark a 
function as unsafe just as a warning of possible dangers in using this function, 
and removing these unsafe will not cause any compile errors (32 or 5% of the 
unsafe usages we studied)." The detailed labels are in column "X" of tab 
"section-4.1-usage". 

Lines 484 - 485. "Five unsafe usages in our studied applications and 56 
in Rust std are for labeling struct constructors." The detailed labels are 
in column "AG" of tab "section-4.1-usage". 

Lines 517 - 519. "We analyzed 108 randomly selected commit logs that 
contain cases where unsafe is removed (130 cases in total)." The detailed 
information of sampled removal cases is available in tab "section-4.2-remove".

Lines 519 - 522. "The purposes of these unsafe code removals include improving 
memory safety (72%), better code structure (19%), improving thread safety (3%), 
bug fixing (3%), and removing unnecessary usages (2%)." The detailed labels 
about why each case is removed are in columns "G" - "K" of tab 
"section-4.2-remove".

Lines 523 - 527. "Among our analyzed commit logs, 55 cases completely change 
unsafe code to safe code. The remaining cases change unsafe code to interior 
unsafe code, with 33 interior unsafe functions in Rust std, 28 self-implemented 
interior unsafe functions, and 14 third-parity interior unsafe functions." 
The detailed labels for each case are listed in columns "M" - "P" of tab 
"section-4.2-remove".

Lines 563 - 565. "For example, 68% of interior unsafe code regions require 
valid memory space or valid UTF-8 characters. 15% require conditions in 
lifetime or ownership." Cases where memory-related checks are conducted 
are labeled in column "J" of tab "section-4.3-interior-std", and cases 
where lifetime/ownership-related checks are conducted are labeled in 
column "O". 

Lines 567 - 569. "Surprisingly, Rust std does not perform any explicit 
condition checking in most of its interior unsafe functions (58%)." Cases 
where developers perform explicit checks are labeled in column "U" of 
tab "section-4.3-interior-std", and cases where no explicit checks are 
conducted are labeled in column "V". 

Lines 576 - 579. "After understanding std interior unsafe functions, 
we inspect 400 sampled interior unsafe functions in our studied 
applications. We have similar findings from these application-written 
interior unsafe functions." The sampled interior unsafe functions are 
listed in tab "section-4.3-interior-std".

Lines 580 - 591. "Worth noticing is that we identified 19 cases where 
interior unsafe code is improperly encapsulated, including five from 
the std and 14 from the applications. Although they have not caused any 
real bugs in the applications we studied, they may potentially cause 
safety issues if they are not used properly. Four of them do not perform 
any checking of return values from external library function calls. Four 
directly dereference input parameters or use them directly as indices 
to access memory without any boundary checking. Other cases include
not checking the validity of function pointers, using type casting to 
change objects’ lifetime to static, and potentially accessing uninitialized 
memory." The identified cases are listed in tab "section-4.3-interior-bad".


5. Memory Safety Issues (Section 4)
---------------------------------------------------------

All numbers in this section are in tab "section-5-memory".

Table 2.  The detailed labels for each bug can be found in columns "F" - 
"T".

Lines 681 - 684. "18 out of 21 bugs in this category follow the same 
pattern: an error happens when computing buffer size or index in safe 
code and an out-of-boundary memory access happens later in unsafe code." 
There are 18 bugs that are labeled with 1 in both column "H" and column 
"N". 

Lines 684 - 690. "For 11 bugs, the effect is inside an interior unsafe 
function. Six interior unsafe functions contain condition checks to 
avoid buffer overflow. However, the checks do not work due to wrong 
checking logic, inconsistent struct status, or integer overflow. For 
three interior functions, their input parameters are used directly 
or indirectly as an index to access a buffer, without any boundary 
checks." The detailed labels are in columns "AA" - "AC". 

Lines 692 - 694. "In five of them, null pointer dereferencing happens 
in an interior unsafe function." The detailed labels are in column "AE".

Lines 698 - 701. "Four of them use unsafe code to create an uninitialized 
buffer and later read it using safe code. The rest initialize buffers 
incorrectly, e.g., using memcpy with wrong input parameters." The detailed 
labels are in columns "AG" and "AH".

Lines 702 - 703. "Out of the ten invalid-free bugs, five share the 
same (unsafe) code pattern." The detailed labels are in column "AJ". 


Lines 728 - 731. "11 out of 14 use-after-free bugs happen because an 
object is dropped implicitly in safe code (when its lifetime ends), 
but a pointer to the object or to a field of the object still exists 
and is later dereferenced in unsafe code." The detailed labels are in 
column "AL".

Lines 743 - 744. "There is one use-after-free bug whose cause and 
effect are both in safe code." The label can be found by referring 
to column "AM".

Lines 746 - 751. "The last two bugs happen in a self-implemented vector. 
Developers explicitly drop the underlying memory space in unsafe code 
due to some error in condition checking. Later accesses to the vector 
in safe code trigger an use-after-free error." The detailed labels are 
in column "AN".

Lines 752 - 754. "There are six double-free bugs. Other than two bugs 
that are safe->unsafe and similar to traditional double-free bugs, 
the rest are all unsafe->safe and unique to Rust." The detailed labels 
are in columns "AP" - "AQ".

Lines 779 - 780. "We examine how our collected memory-safety bugs were 
fixed and categorize their fixing strategies into four categories." 
The detailed strategies are labeled in columns "V" - "Y".

Lines 786 - 788. "25 of these bugs were fixed by skipping unsafe code, 
two were fixed by skipping interior unsafe code, and three skipped safe 
code." The detailed labels are in columns "AS" - "AU". 



6. Blocking Bugs (Section 6.1)
----------------------------------------

All numbers in this section are in tab "section-6.1-blocking", unless 
otherwise specified. 

Table 3. The detailed labels are in columns "H" - "T". 

Lines 866 - 870. "Failing to acquire Lock (for Mutex) or read/write 
(for RwLock) results in thread blocking for 38 bugs, with 30 of them 
caused by double locking, seven caused by acquiring locks in conflicting 
orders, and one caused by forgetting to unlock when using a 
self-implemented mutex." Bugs caused by double locks are labeled in column 
"AC". Bugs caused by acquiring locks in conflicting orders are labeled 
in column "AG". Bugs caused by forgetting to unlock are labeled in 
column "AH". 


Lines 906 - 910. "in five double-lock bugs, the first lock is in a match 
condition and the second lock is in the corresponding match body (e.g., 
Figure 6). In another five double-lock bugs, the first lock is in an 
if condition, and the second lock is in the if block or the else block." 
The detailed labels are in columns "AD" and "AE". 

Lines 915 - 920. "In eight of the ten bugs related to Condvar, one thread 
is blocked at wait() of a Condvar, while no other threads invoke notify_one() 
or notify_all() of the same Condvar. In the other two bugs, one thread 
is waiting for a second thread to release a lock, while the second thread 
is waiting for the first to invoke notify_all()." The detailed labels are 
in columns "J" and "K".

Lines 923 - 924. "There are five bugs caused by blocking at receiving 
operations." The detailed labels are in column "M". 

Lines 934 - 937. "There is one bug that is caused by a thread being 
blocked when sending to a full channel." The detailed labels are in column "N". 

Lines 946 - 947. "We have one bug of this type." The label is in column "P".

The detailed labels for how each bug is fixed are in columns "V" - "Z". 

Lines 960 - 961. "This strategy was used for the bug of Figure 6 and 
16 other bugs." The detailed labels are in column "AA".

Lines 969 - 972. "We found 11 such usages in our studied applications. 
Among them, nine cases perform explicit drop to avoid double lock and 
one case is to avoid acquiring locks in conflicting orders." All these 
cases are listed in tab "section-6.1-drop". We built a tool to search 
these cases, and the tool can be executed on the virtual machine using the following command:
```
cd ~/pldi-2020/rust-study/section-6-thread-safety-issues/section-6-1-blocking-bugs
./run_all.sh
```


7. Non-Blocking Bugs (Section 6.2)
------------------------------------------------

All numbers in this section are in tab "section-6.1-non-blocking", 
unless otherwise specified. 

Table 4. Detailed labels are in columns "F" - "R". 

Lines 1018 - 1019. "out of which 20 use interior-unsafe functions to 
share data." The detailed labels for whether an unsafe function or an 
interior unsafe function is used are in columns "N" - "O". 

Lines 1052 - 1054. "To ensure lifetime covers all usages, eight bugs 
use Arc to wrap shareddata and the other six bugs use global variables 
as shared variables." The detailed labels are in columns "T" - "X".  

Lines 1067 - 1073. "15 of them do not synchronize (protect) the shared 
memory accesses at all, and the memory is shared using unsafe code. 
This result shows that using unsafe code to bypass Rust compiler checks 
can severely degrade concurrency safety of Rust programs. 22 of them 
synchronize their shared memory accesses, but there are issues in 
the synchronization." The detailed labels are in columns "Z" - "AA". 

Lines 1110 - 1111. "Improper use of interior mutability can cause 
non-blocking bugs (14 in total in our studied set)." The detailed 
labels are in column "AE"

How to fix non-blocking bugs are labeled in columns "AG" - "AK".



8. Bug Detection (Section 7)
------------------------------------------------

Our use-after-free detector can be executed using the following 
commands on the virtual machine: 
```
cd ~/pldi-2020/rust-study/section-7-bug-detection/section-7.1-detecting-memory-bugs
./run_uaf_detector.sh
```

Its detailed document can be found here: 
https://github.com/system-pclub/rust-study/tree/master/section-7-bug-detection/section-7.1-detecting-memory-bugs/use-after-free-detector

All identified UAF bugs are reported in the following pull request: 
https://gitlab.redox-os.org/redox-os/relibc/issues/159


Our double-lock detector can be executed using the following commands on the virtual machine:
```
cd section-7-bug-detection/section-7.2-detecting-concurrency-bugs/double-lock-detector
./run_all.sh
```

Its detailed document can be found here: 
https://github.com/system-pclub/rust-study/tree/master/section-7-bug-detection/section-7.2-detecting-concurrency-bugs/double-lock-detector

Our identified bugs are reported in the following pull requests: 
https://github.com/OpenEthereum/open-ethereum/pull/11172
https://github.com/OpenEthereum/open-ethereum/pull/11175
https://github.com/OpenEthereum/open-ethereum/issues/11176
