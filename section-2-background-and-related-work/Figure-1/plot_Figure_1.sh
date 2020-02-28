#!/bin/sh
ploticus plot_rust_histories.pl -eps -o Figure_1.eps -textsize 18 -font /Courier
epstopdf Figure_1.eps
rm -rf Figure_1.eps
