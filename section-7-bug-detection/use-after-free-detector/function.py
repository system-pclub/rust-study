import basic_block
from variable import *
from statement_parser import StatementParser
import queue


class Function:
    def __init__(self, name, args):
        self.name = name
        self.basic_blocks = []
        self.bb_idx = -1
        self.args = {}
        self.local_variables = {}
        self.global_variables = {}
        self.paths = []
        for arg_name, arg_type in args:
            self.add_args(arg_name, arg_type)

    def set_basic_block(self, line):
        name = line.split()[0].split(':')[0]
        bb = basic_block.BasicBlock(name)
        self.basic_blocks.append(bb)
        self.bb_idx += 1

    def set_statement(self, statement):
        assert(self.bb_idx >= 0)
        self.basic_blocks[self.bb_idx].add_statement(statement)

    @staticmethod
    def find_basic_block_by_name(bb_name, bb_vector):
        for bb in bb_vector:
            if bb.get_name() == bb_name:
                return bb
        return None

    def add_args(self, arg_name, arg_type):
        arg = self.add_local_variable(arg_name, arg_type)
        self.args[arg_name] = arg

    def add_local_variable(self, variable_name, variable_type):
        v = Variable(variable_name, variable_type)
        self.local_variables[variable_name] = v
        return v

    def find_local_variable_by_name(self, variable_name):
        if variable_name in self.local_variables:
            return self.local_variables[variable_name]
        else:
            return None

    def add_global_variable(self, variable_name, variable_type):
        v = Variable(variable_name, variable_type)
        self.global_variables[variable_name] = v
        return v

    def find_global_variable_by_name(self, variable_name):
        if variable_name in self.global_variables:
            return self.global_variables[variable_name]
        else:
            return None

    def reset_variables_state(self):
        for v in self.local_variables.values():
            v.reset()
        for v in self.global_variables.values():
            v.reset()

    def flatten_cfg(self):
        # This function is used to generate all the paths of control flow but without loopback
        root = self.basic_blocks[0]
        current_flow = []
        idx = 0
        current_flow.append(root)
        self.paths.append(current_flow)

        while idx != len(self.paths):
            current_flow = self.paths[idx]
            last_bb = current_flow[-1]
            successors = last_bb.find_successors()
            need_remove_flow = False

            for succ in successors:
                # make sure there is no loopback
                bb = self.find_basic_block_by_name(succ, current_flow)
                if bb is not None:
                    continue

                bb = self.find_basic_block_by_name(succ, self.basic_blocks)
                if bb is None:
                    print(succ)
                assert(bb is not None)
                new_flow = current_flow.copy()
                new_flow.append(bb)
                self.paths.append(new_flow)
                need_remove_flow = True

            if need_remove_flow:
                self.paths.remove(current_flow)
                idx = 0
                continue

            idx += 1

    def traverse_control_flow_graph_fast(self):
        current_path = []
        root = self.basic_blocks[0]
        idx = 0
        current_path.append(root)
        self.paths.append(current_path)

        while idx != len(self.paths):
            current_path = self.paths[idx]
            last_bb = current_path[-1]
            successors = last_bb.find_successors()
            need_remove_flow = False

            for successor in successors:
                # make sure there is no loopback
                bb = self.find_basic_block_by_name(successor, current_path)
                if bb is not None:
                    continue

                bb = self.find_basic_block_by_name(successor, self.basic_blocks)
                assert(bb is not None)
                if bb.marked > 2:
                    continue

                bb.marked += 1
                new_path = current_path.copy()
                new_path.append(bb)
                self.paths.append(new_path)
                need_remove_flow = True

            if need_remove_flow:
                self.paths.remove(current_path)
                idx = 0
                continue

            idx += 1

    def parser_statements(self):
        """
        Test version
        """

        """
        flow = self.flows[-1]
        for bb in flow:
            statements = bb.get_statements()
            for s in statements:
                statement_parser = StatementParser(self, s)
                statement_parser.parser_statement()
        self.detect_use_after_free_final(self.global_variables)
        """

        # """
        for flow in self.paths:
            self.reset_variables_state()
            for bb in flow:
                statements = bb.get_statements()
                for s in statements:
                    statement_parser = StatementParser(self, s)
                    statement_parser.parser_statement()

            for variable in self.global_variables.values():
                self.detect_dangling_pointer_recursive(variable)

            # The arg itself can be a dangling pointer since the function call arguments are value-copied
            for arg in self.args.values():
                if len(arg.children) > 0:
                    for child in arg.children:
                        self.detect_dangling_pointer_recursive(child)
        # """

    def detect_dangling_pointer_recursive(self, variable):
        if len(variable.children) > 0:
            for child in variable.children:
                self.detect_dangling_pointer_recursive(child)

        if variable.is_dangling_pointer():
            print("Use-after-free detected: source variable: " + variable.name +
                  " is a dangling pointer and global accessible, it points to: " + variable.reference_to.name)

    """
    dump information
    """
    def dump_flows(self):
        for flow in self.paths:
            for bb in flow:
                print(bb.get_name() + ' -> ', end='')
            print('')

    def dump_variables(self):
        for v in self.local_variables:
            v.dump()

    def dump(self):
        print("Function: " + self.name)
        for bb in self.basic_blocks:
            bb.dump()
