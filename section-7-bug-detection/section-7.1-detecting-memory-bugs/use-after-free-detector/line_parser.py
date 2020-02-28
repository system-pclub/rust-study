from function import Function
from utils import *
import logging


class LineParser:
    def __init__(self, filepath):
        self.filepath = filepath
        self.function = None
        self.parsing_function = False
        self.parsing_basic_block = False

    def run(self):
        file = open(self.filepath)
        lines = file.readlines()
        for line in lines:
            self.parse_line(line)

        if self.function is not None:
            self.function.flatten_cfg()
            # self.function.traverse_control_flow_graph_fast()
            self.function.parser_statements()

    def parse_line(self, line):
        if self.is_empty_line(line):
            return
        elif self.is_comment(line):
            return
        elif self.is_function_declaration(line):
            self.set_function(line)
            self.parsing_function = True
            return

        if self.parsing_function:
            if self.is_basic_block_declaration(line):
                self.function.set_basic_block(line)
                self.parsing_basic_block = True
                return

            if self.parsing_basic_block:
                if self.is_end(line):
                    self.parsing_basic_block = False
                    return
                else:
                    # This is a statement, and should be added to current basic block
                    self.function.set_statement(line)
            else:
                # It is parsing function declaration part.
                if self.is_variable_declaration(line):
                    self.set_variable(line)

    def set_function(self, line):
        right = line[line.find('fn ') + 3:]
        name = right.split('(')[0]
        args = []

        # The leading '(' and tailing ')' is removed now
        args_str = right[right.find('('): right.rfind(')')].strip('(')
        pattern = r'.(?=_\d+: )'
        tokens = re.split(pattern, args_str)
        if tokens:
            pattern = r'(_\d+): (.+(?!\, ))'
            for token in tokens:
                m = re.search(pattern, token)
                if m:
                    arg_name = str(m.group(1))
                    arg_type = str(m.group(2))
                    args.append((arg_name, arg_type))
                    logging.debug('Adding arg (name: %s, type: %s) to function %s', arg_name, arg_type, name)

        self.function = Function(name, args, self.filepath)

    def set_variable(self, line):
        line = line.split('//')[0]
        var_name = line.split(': ')[0].split()[-1]
        var_type_name = line.split(': ')[1].split(';')[0]
        self.function.add_local_variable(var_name, var_type_name)

    @staticmethod
    def is_empty_line(line):
        tokens = line.split()
        if len(tokens) == 0:
            return True
        else:
            return False

    # return true if this line is comment
    @staticmethod
    def is_comment(line):
        tokens = line.split()
        if len(tokens) == 0:
            return False
        if tokens[0] == '//':
            return True
        else:
            return False

    @staticmethod
    def is_function_declaration(line):
        tokens = line.split()
        if tokens[0] == 'fn' and tokens[-1] == '{':
            return True
        if tokens[0] == 'pub' and tokens[1] == 'fn' and tokens[-1] == '{':
            return True
        return False

    @staticmethod
    def is_basic_block_declaration(line):
        tokens = line.split()
        if tokens[0].startswith('bb') and tokens[-1] == '{':
            return True
        return False

    @staticmethod
    def is_end(line):
        tokens = line.split()
        if tokens[0] == '}':
            return True
        return False

    @staticmethod
    def is_variable_declaration(line):
        tokens = line.split()
        if tokens[0] == 'let':
            return True
        return False