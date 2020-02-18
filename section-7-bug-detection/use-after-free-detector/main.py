import sys
import logging
import subprocess
import line_parser

skipping_file_list = ['rustc.header-stdio-printf-inner_printf.003-027.PreCodegen.after.mir',
                      'rustc.header-stdio-scanf-inner_scanf.003-027.PreCodegen.after.mir',
                      'rustc.header-stdio-scanf-inner_scanf.003-027.PreCodegen.after.mir']


def find_mir_files(mir_dir):
    mir_files = []
    proc = subprocess.Popen(['find', mir_dir, '-name',  '*PreCodegen.after.mir'], stdout=subprocess.PIPE)
    lines = proc.stdout.readlines()
    for line in lines:
        line = line.decode()[0: -1]
        mir_files.append(line)
    return mir_files


def file_should_be_skipped(filename):
    for f in skipping_file_list:
        if filename.endswith(f):
            return True
    return False


if __name__ == '__main__':
    '''
    Setup logger
    @ console: print critical level and above
    @ file: print debug level and above
    '''
    logger = logging.getLogger()
    logger.setLevel(logging.DEBUG)

    formatter = logging.Formatter('%(asctime)s - %(levelname)s - %(message)s')

    fh = logging.FileHandler('detector.log')
    fh.setLevel(logging.CRITICAL)
    fh.setFormatter(formatter)
    logger.addHandler(fh)

    ch = logging.StreamHandler()
    ch.setLevel(logging.ERROR)
    ch.setFormatter(formatter)
    # logger.addHandler(ch)

    logging.info('logger is setup.')

    mir_files = find_mir_files(sys.argv[1])
    nr_files_parsed = 0

    for file in mir_files:
        if file_should_be_skipped(file):
            continue

        logging.info('Parsing MIR file: %s', file)
        print('NR_FILES parsed: ' + str(nr_files_parsed) + ', parsing MIR file: ' + file)
        parser = line_parser.LineParser(file)
        parser.run()
        nr_files_parsed += 1