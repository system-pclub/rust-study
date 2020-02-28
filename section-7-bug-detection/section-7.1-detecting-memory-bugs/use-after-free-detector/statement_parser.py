from define_types import *
from utils import *
from variable import *
import logging

ptr_functions = ['as_ptr', 'as_mut_ptr']
skipping_functions = ['discriminant', 'Not', 'Eq', 'Box', 'Gt', 'CheckedSub', 'Lt', 'Len', 'Div', 'Ne', 'Ge', 'Le',
                      'BitOr', 'CheckedAdd', 'BitAnd', 'Rem', 'CheckedMul', 'CheckedShr', 'CheckedShl', '[]', 'Mul',
                      'Sub', 'Add']


class StatementParser:
    def __init__(self, function, statement):
        self.statement = statement.split('//')[0]
        self.function = function
        self.statement_type = StatementType.Unset

    def parser_statement(self):
        if self.is_assignment(self.statement):
            self.statement_type = StatementType.Assignment
            tokens = self.statement.split(' = ')

            dest_variable, dest_assignment_type = self.find_destination_variable()
            if dest_variable is None:
                return

            # Don't do again in callees
            dest_variable.set_lifetime_state(LifetimeState.Alive)

            if self.should_skip(tokens[0]) or self.should_skip(tokens[1]):
                return

            src_str = tokens[1]
            if self.is_function_call(src_str):
                operands = self.get_function_operands()
                num_operands = len(operands)

                self.handle_mem_forget(operands)

                # Now I can only handle one case: dest is reference/pointer, src is reference/pointer
                # and the num_operands == 1, consider dest == src
                if dest_variable.get_type() == VariableType.Pointer or \
                        dest_variable.get_type() == VariableType.Reference:
                    if num_operands == 1:
                        src_variable = operands[0]

                        if src_variable.get_type() == VariableType.Pointer or \
                                src_variable.get_type() == VariableType.Reference:
                            set_reference(dest_variable, src_variable.reference_to)
                            logging.debug('Function call, set %s identical as %s',
                                          dest_variable.name, src_variable.name)
            else:
                source_variable_vector = self.find_source_variables()
                num_source_variables = len(source_variable_vector)

                if num_source_variables > 1:
                    idx = 0

                    for src_variable, src_assignment_type, moved in source_variable_vector:
                        child_type_name = src_variable.type_name
                        child_variable = dest_variable.find_child_variable_by_name(str(idx))

                        # if not find, initialize one
                        if child_variable is None:
                            child_variable = Variable(str(idx), child_type_name)
                            dest_variable.add_child_variable(child_variable)

                        self.do_single_variable_assignment(child_variable, dest_assignment_type,
                                                           src_variable, src_assignment_type, moved)
                        idx += 1
                        logging.debug('Multiple value assignment, src: %s, dest: %s, member: %s',
                                      src_variable.name, dest_variable.name, child_variable.name)

                # one source variable, directly invoke
                elif num_source_variables == 1:
                    src_variable, src_assignment_type, moved = source_variable_vector[0]
                    self.do_single_variable_assignment(dest_variable, dest_assignment_type,
                                                       src_variable, src_assignment_type, moved)
                # const assignment
                else:
                    self.do_single_variable_assignment(dest_variable, dest_assignment_type,
                                                       None, AssignmentType.Regular, False)

        if self.statement.startswith('StorageDead'):
            self.statement_type = StatementType.TerminateLifetime
            operand = self.statement.split('(')[1].split(')')[0]

            variable = self.function.find_local_variable_by_name(operand)
            assert(variable is not None)
            if variable.get_lifetime_state() == LifetimeState.Forgot:
                logging.debug("Don't drop variable: %s, because it is forgot", variable.name)
            else:
                variable.set_lifetime_state(LifetimeState.Terminated)

    '''
    dest_variable and src_variable must be final before calling this function
    src_variable is not from function call
    '''
    def do_single_variable_assignment(self, dest_variable, dest_assignment_type,
                                      src_variable, src_assignment_type, src_moved):
        if dest_variable.get_type() == VariableType.Scalar:
            return

        if dest_variable.get_type() == VariableType.Object:
            # p = 0
            if src_variable is None:
                logging.debug('Assign droppable with a const: %s', self.statement.strip())
                return

            # p = q
            if src_variable.get_type() == VariableType.Object:
                if src_moved:
                    self.handle_moving_recursive(src_variable, dest_variable)
                    logging.debug('Moved variable %s to variable: %s', src_variable.name, dest_variable.name)
                else:
                    logging.warning('Assign Object with Object: %s', self.statement.strip())

            # p = *q
            if src_variable.get_type() == VariableType.Reference or src_variable.get_type() == VariableType.Pointer:
                # assert(src_assignment_type == AssignmentType.Dereference)
                logging.debug('Assign Object with pointer/reference: %s', self.statement.strip())

        if dest_variable.get_type() == VariableType.Reference or dest_variable.get_type() == VariableType.Pointer:
            if src_variable is None:
                if dest_assignment_type == AssignmentType.Dereference:
                    logging.debug('Assign pointer/reference internal value with const')
                else:
                    logging.debug('Assign pointer/reference with const')
                return

            # *p = 0 or p = &0
            if src_variable.get_type() == VariableType.Scalar:
                if dest_assignment_type == AssignmentType.Dereference:
                    # *p = 0, nothing special is needed
                    logging.debug('Assign reference/pointer with *p = a: %s', self.statement.strip())
                else:
                    if src_assignment_type != AssignmentType.Reference:
                        """
                        This is casting from addr to pointer, no need to handle
                        """
                        logging.debug('Assign pointer/reference by casting from an address')
                    else:
                        set_reference(dest_variable, src_variable)
                        logging.debug('Assign reference/pointer with reference: %s', self.statement.strip())

            # *p = q or p = &q
            if src_variable.get_type() == VariableType.Object:
                # p = &q
                if dest_assignment_type == AssignmentType.Regular:
                    # print(self.statement.strip())
                    # assert(src_assignment_type == AssignmentType.Reference)
                    set_reference(dest_variable, src_variable)
                    logging.debug('dest_variable: %s, type: %s, points to %s now!!',
                                  dest_variable.name, str(dest_variable.get_type()), src_variable.name)
                # *p = q
                else:
                    assert(dest_assignment_type == AssignmentType.Dereference)
                    assert(src_assignment_type == AssignmentType.Regular)

                    if dest_variable.reference_to is not None:
                        dest_variable.reference_to.set_lifetime_state(StatementType.TerminateLifetime)
                        set_reference(dest_variable, src_variable)
                    else:
                        logging.critical('Dropping uninitialized memory? : %s', self.statement.strip())

            # p = q (p, q are both pointer or reference)
            if src_variable.get_type() == VariableType.Reference or src_variable.get_type() == VariableType.Pointer:
                set_reference(dest_variable, src_variable.reference_to)
                logging.debug('dest_variable: %s, type: %s, is set to %s now!!',
                              dest_variable.name, str(dest_variable.get_type()), src_variable.name)

    """
    @ return value: dest_variable, deref? 
    """
    def find_destination_variable(self):
        dest_str = self.statement.split(' = ')[0]
        dest_variable, assignment_type, moved = self.find_single_variable(dest_str)
        assert(not moved)
        return dest_variable, assignment_type

    """
    @ return value: [(variable_0, moved?, by_ref?, single?), ..., (variable_N, moved?, by_ref?)]
    """
    def find_source_variables(self):
        source_variables = []
        src_str = self.statement.split(' = ')[1].strip().strip(';')
        if self.is_function_call(src_str):
            logging.error('Finding function call operands should not go here')
            return source_variables
        elif self.is_const(src_str):
            logging.debug('Source variable is const: %s', self.statement.strip())
            return source_variables
        elif self.is_variable_vector(src_str):
            return self.find_multiple_variables(src_str)
        else:
            if not self.should_skip(src_str):
                source_variables.append(self.find_single_variable(src_str))

        '''
        detect using dangling pointer as source variable
        '''
        for src_variable, _, _ in source_variables:
            if src_variable.is_dangling_pointer():
                print('Use-after-free detected: using dangling pointer: ', src_variable.name,
                      ' as source variable, it points to: ', src_variable.reference_to.name, " in file: ",
                      self.function.filepath)

        return source_variables

    """
    @ search_str: must remove leading space, tailing space and ';' before invoking
    @ search str should not started with `discriminant`
    @ return value: (variable, moved?, reference?, dereference?)
    """
    def find_single_variable(self, search_str):
        root = True
        moved = False

        if search_str.startswith('move '):
            moved = True
            search_str = search_str.strip('move ')

        assignment_type, variable_vector = find_variable_name_and_type(search_str)
        variable = None

        for variable_name, variable_type in variable_vector:
            if root:
                '''
                Parsing the root of this variable
                '''
                if is_local_variable(variable_name):
                    variable = self.function.find_local_variable_by_name(variable_name)
                    assert(variable is not None)
                else:
                    variable = self.function.find_global_variable_by_name(variable_name)
                    if variable is None:
                        assert (variable_type is not None)
                        # The global variable is created now.
                        variable = self.function.add_global_variable(variable_name, variable_type)

                root = False
                # print('root variable name: ' + variable_name)
            else:
                """
                Now the variable should be parent variable
                """
                # print(child_variable_name)
                child_variable = variable.find_child_variable_by_name(variable_name)
                if child_variable is None:
                    assert(variable_type is not None)
                    child_variable = Variable(variable_name, variable_type)
                    variable.add_child_variable(child_variable)
                else:
                    if variable_type is not None:
                        """
                        This is a very tricky case that the same member in enum type can be a different type
                        """
                        old_type_name = child_variable.type_name
                        old_type = child_variable.get_type()
                        child_variable.reset_type(variable_type)
                        logging.warning('Change the variable type, old_type_name: %s, old type: %s, '
                                        'new_type_name: %s, new_type: %s',
                                        old_type_name, str(old_type), variable_type, str(child_variable.get_type()))

                # do recursively
                variable = child_variable

        """ logging """
        if variable.get_type() == VariableType.Pointer:
            if variable.reference_to is not None:
                logging.debug('Find pointer variable: variable: %s, assignment_type: %s, moved: %s, reference_to: %s, '
                              'reference_to.type: %s, reference_to.state: %s',
                              variable.name, str(assignment_type), str(moved), variable.reference_to.name,
                              variable.reference_to.get_type(), variable.reference_to.get_lifetime_state())
            else:
                logging.debug('Find pointer variable: variable: %s, assignment_type: %s, moved: %s, '
                              'reference_to is None', variable.name, str(assignment_type), str(moved))
        else:
            logging.debug('Find non-pointer variable: variable: %s, assignment_type: %s, moved: %s',
                          variable.name, str(assignment_type), str(moved))

        return variable, assignment_type, moved

    def find_multiple_variables(self, search_str):
        variable_vector = []
        tokens = search_str.split(', ')
        for v in tokens:
            variable_vector.append(self.find_single_variable(v.strip('[').strip(']')))

        return variable_vector

    """
    Functions in this part is used to handle function calls
    """
    def get_function_operands(self):
        operands = []
        src_str = self.statement.split(' = ')[1].strip().strip(';').split(' -> ')[0].strip()
        """
        Finding variables in function call is easy, because it must start with 'move '
        """
        tokens = src_str.split('move ')

        # There is no variable for function call
        if len(tokens) == 1:
            logging.debug('function call: %s, operands is empty', src_str)
        else:
            tokens = tokens[1:]
            for token in tokens:
                variable_str = token.split(', ')[0].strip().strip(')').strip()
                assert is_local_variable(variable_str)

                _, variable_vector = find_variable_name_and_type(variable_str)
                variable = None
                root = True

                for variable_name, variable_type in variable_vector:
                    if root:
                        variable = self.function.find_local_variable_by_name(variable_name)
                        root = False
                    else:
                        child_variable = variable.find_child_variable_by_name(variable_name)
                        if child_variable is None:
                            assert (variable_type is not None)
                            child_variable = Variable(variable_name, variable_type)
                            variable.add_child_variable(child_variable)

                        # do recursively
                        variable = child_variable

                # assert(variable is not None)
                operands.append(variable)

                logging.debug("Function call: %s, getting one function call operand: %s", src_str, variable.name)

        return operands

    """
    This function do the following things
    1. check if the callee is mem::forget
    2. If 1 is true, set the object lifetime state as forgot
    """
    def handle_mem_forget(self, operands):
        pattern = r'mem::forget'
        src_str = self.statement.split(' = ')[1].strip().strip(';').split(' -> ')[0].strip()

        m = re.search(pattern, src_str)

        if m:
            assert(len(operands) == 1)
            self.do_forget_recursive(operands[0])

    '''
    set variable and its child lifetime state as terminate recursively
    '''
    def do_forget_recursive(self, variable):
        if len(variable.children) > 0:
            for child in variable.children:
                self.do_forget_recursive(child)

        variable.set_lifetime_state(LifetimeState.Forgot)
        logging.debug('Forget variable: %s', variable.name)

    '''
    Moving src_variable to dest_variable
    All reference to the children of src_variable, and src_variable itself should
    reference to destination now
    '''
    def handle_moving_recursive(self, src_variable, dest_variable):
        if len(src_variable.children) > 0:
            for child in src_variable.children:
                self.handle_moving_recursive(child, dest_variable)

        for ref in src_variable.referenced_by:
            set_reference(ref, dest_variable)

            logging.debug('%s reference_to is reset, old: %s, new: %s', ref.name,
                             src_variable.name, dest_variable.name)

    @staticmethod
    def is_assignment(statement):
        if ' = ' in statement:
            return True
        return False

    @staticmethod
    def is_function_call(search_str):
        if search_str.startswith('const ') and ' -> ' in search_str:
            return True
        else:
            return False

    @staticmethod
    def is_const(search_str):
        first = search_str.split()[0]
        if 'const' in first and ' -> ' not in search_str:
            return True
        else:
            return False

    @staticmethod
    def is_variable_vector(search_str):
        if 'const' in search_str:
            return False
        elif search_str.startswith('[') and search_str.endswith(']'):
            return True
        else:
            return False

    @staticmethod
    def should_skip(search_str):
        for s in skipping_functions:
            if search_str.startswith(s):
                return True
        return False
