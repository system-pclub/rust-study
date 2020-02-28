#!/bin/sh
ploticus plot_bugs_date.pl -eps -o Figure_2.eps -textsize 18 -font /Courier
epstopdf Figure_2.eps
rm -rf Figure_2.eps
