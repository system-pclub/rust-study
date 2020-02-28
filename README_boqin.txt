# Sec 3
Line 418. The number in “In total, we studied 70 memory and 100 concurrency bugs.” can be got by referring to `artifact.xlsx`, section-5-memory tab: column A, section-6.1-blocking tab: column A and section-6.2-non-blocking tab: column A.

# Sec 4
Line 428-434. The numbers in “We found 12835 unsafe usages in our studied applications in
Table 1, including 7061 unsafe code regions, 5727 unsafe functions, and 47 unsafe traits. In Rust’s standard library (Rust std for short), we found 1577 unsafe code regions, 870
unsafe functions, and 12 unsafe traits.” can be got by running the following command:
```
cd  ~/Projects/Rust/rust-study/section-4-unsafe-usages/unsafe-statisitcs/src_parser
./run_all.sh
```

## Sec 4.1
Line 436-442. The numbers in “We randomly select 600 unsafe usages from our studied applications” can be got by referring to `artifact.xlsx`, section-4.1-usage tab: A3:A610.
The numbers in “including 400 interior unsafe usages and 200 unsafe functions.” in section-4-3-interior tab: A276:A683
The numbers in “We also studied 250 interior unsafe usages in Rust std.” in section-4-3-interior tab: A3:A252

Line 451-453. “Most of them (66%) are for (unsafe) memory operations, such as raw pointer manipulation and type casting. Calling unsafe functions counts for 29%” can be got by referring to `artifact.xlsx`, section-4.1-usage tab: column F, column P.

Line 454-456. “Most of these calls are made to unsafe functions programmers write themselves and functions written in other languages.” can be got by referring to `artifact.xlsx`, section-4.1-usage tab column T, column R.

Line 460-478. “purposes of our studied 600 unsafe usages.” and the percentage can be got by referring to `artifact.xlsx`, section-4.1-usage tab column Z-AE.

Line 466-475. “performance” can be got by running the following command:
```
cd  ~/Projects/Rust/rust-study/section-4-unsafe-usages/section-4-1-reasons-of-usage/
./run_all.sh
```

Line 482. “(32 or 5%)” can be got by referring to `artifact.xlsx`, section-4.1-usage tab column X.

Line 484. “Five unsafe usages in our studied applications and 56 in” can be got by referring to `artifact.xlsx`, section-4.1-usage tab column AG.

## Sec 4.2
Line 517-581. “We analyzed 108 randomly selected commit logs that contain cases where unsafe is removed (130 cases in total).” can be got by referring to `artifact.xlsx`, section-4.2-remove tab: column C and column D.

Line 519-522. “The purposes of these unsafe code removals” and the percentage can be got by referring to `artifact.xlsx`, section-4.2-remove tab: column G and column K.

Line 523-527. “55 cases...” can be got by referring to `artifact.xlsx`, section-4.2-remove tab: column M to P.

## Sec 4.3
Line 561-565. “we sampled 250 interior unsafe functions”, “68%”, “15%” can be got by referring to `artifact.xlsx`, section-4.2-interior tab: column B, T, U.

Line 569. “58%” can be got by referring to `artifact.xlsx`, section-4.2-interior tab: column H.

Line 577. “400” can be got by referring to `artifact.xlsx`, section-4.2-interior tab: B685.

Line 580-582. “19 cases where interior unsafe code is improperly encapsulated, including five from the std and 14 from the applications.” can be got by referring to `artifact.xlsx`, section-4.2-interior tab: A690 to P179

# Sec 6
Line 818. The number in “our 100 collected concurrency bugs” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column A. section-6.2-non-blocking tab: column A.

## Sec 6.1
Table 3. can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: H95 to R102.

Line 837. The number in “In total, we studied 59” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column A.

Line 845-850. The numbers in “55 out of 59 blocking bugs are caused by operations of synchronization primitives”, “The other four bugs” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column H, J, M, N, P and column R, S, T.

Line 866-870. The numbers in “Failing to acquire Lock… for 30 bugs, with 30
of them caused by double locking, seven caused by acquiring locks in conflicting orders, and one caused by forgetting to unlock when using a self-implemented mutex.” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column H, J, M, N, P and column R, S, T.

Line 866-870. The numbers in “Failing to acquire Lock… for 30 bugs, with 30
of them caused by double locking, seven caused by acquiring locks in conflicting orders, and one caused by forgetting to unlock when using a self-implemented mutex.” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column H, J, M, N, P and column R, S, T.

Line 906-910. The numbers in “in five double-lock bugs, the first lock is in a match condition ...In another five double-lock bugs, the first lock is in an if condition...” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column AD and column AE.

Line 915-918. The numbers in “In eight of the ten bugs related to Condvar” and “In the other two bugs” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column J and column K.

Line 923-929. The numbers in “There are five bugs caused by blocking at receiving operations.”, “In one bug”, “For another three bugs”, “In the last bug” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column M. (?)

Line 934. The number in “There is one bug ” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column N.

Line 945. The number in “We have one bug of this type” for once can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: column P.

Line 955. The number in “collected (50/59) were fixed by adjusting synchronization operations,” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: AA95.

Line 961. The number in “for the bug of Figure 6 and 16 other bugs” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: Column AA.

Line 964. The number in “The other nine blocking bugs were not fixed by adjusting
synchronization mechanisms.” can be got by referring to `artifact.xlsx`, section-6.1-blocking tab: Column Z.

Line 969-972. The numbers in “We found 11 such usages in our studied applications. Among
them, nine cases perform explicit drop to avoid double lock and one case is to avoid acquiring locks in conflicting orders.” can be got by running the following command:
```
cd  ~/Projects/rust-study/section-6-thread-safety-issues/section-6-1-blocking-bugs
./run_all.sh
```

Output:

```
Manual Drop Info:
 /home/boqin/Projects/Rust/double-lock/parity-ethereum util/network-devp2p/src/host.rs 378
         /home/boqin/Projects/Rust/double-lock/parity-ethereum util/network-devp2p/src/host.rs 382
...
```

The first source code location is the lock.
The second source code location is where the lock is manually dropped.

Reported Cases and Analysis:
In manual_drop.xlsx

## Sec 6.2
Line 987. The numbers in “Among the 41non-blocking bugs, four are caused by errors in message passing” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column F and G.

Table 4. can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: F55 to L62 and Q55 to R62.

Line 1015. The number in “how the 37 non-blocking bugs share data” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column G.

Line 1018-1019. The numbers in “23 non-blocking bugs share data using unsafe code, out of which 20 use interior-unsafe functions to share data.” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column N and column O.

Line 1024-1025. The number in “share data is by passing a raw pointer to a memory space (13…)”
can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column K.

Line 1030-1031. The number in “the second most common type of data sharing (5)”
can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column K.

Line 1012-1031. The number in “The other two unsafe data-sharing methods used in the remaining 5 bugs” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column I and column L.

Line 1048. The number in “14 non-blocking bugs share data with safe code” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column I and column L.

Line 1050-1051. The numbers in “five of them use atomic variables, and the other nine bugs” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column Q and column R.

Line 1053-1054. The numbers in “eight bugs use Arc to wrap shared data and the other six bugs use global variables” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column T and column W, column U and column X.

Line 1067. The number in “15 of them do not synchronize” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column AA.

Line 1071. The number in “ 22 of them synchronize their shared memory accesses” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column Z.

Line 1111 and Line 1133. The number in “14 in total in our studied set” and “There are 13 more non-blocking bugs” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column AE.

Line 1151-1158. The number in “19 bugs are fixed by adjusting synchronization primitives”, “Nine are fixed by enforcing ordering”, “Five are fixed by avoiding (problematic) shared memory accesses”, “ One is fixed by making a local copy of some shared memory”,  “Finally, three are fixed by changing application-specific logic” can be got by referring to `artifact.xlsx`, section-6.2-non-blocking tab: column AG-AK.

## Sec 7.2
The six previously unknown double-lock bugs on line 1280 can be got by running the following command:
```
cd  ~/Projects/rust-study/section-7-bug-detection/section-7-2-detecting-concurrency-bugs/double-lock-detector
./run.sh ~/Projects/double-lock-bc/ethereum-93fbbb9aaf161f21471050a2a3257f820c029a73/m2r```

Output:
```
Double Lock Happens! First Lock:
start: /home/boqin/Projects/Rust/double-lock/parity-ethereum ethcore/src/client/client.rs 1947
Second Lock(s):
bb110: /home/boqin/Projects/Rust/double-lock/parity-ethereum ethcore/src/client/client.rs 2039
...
```

The output is in `double_lock.log`, which records the BasicBlock name and source code location of the first and second lock,
followed by a call chain from the second lock to the first one.


Reported Bugs:

https://github.com/paritytech/parity-ethereum/pull/11172 (1 bug)
https://github.com/paritytech/parity-ethereum/pull/11175 (1 bug)
https://github.com/paritytech/parity-ethereum/issues/11176 (4 bugs)


