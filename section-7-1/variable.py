from define_types import VariableType
from define_types import LifetimeState
import logging


scalars = ['i8', 'i16', 'i32', 'i64', 'u8', 'u16', 'u32', 'u64', 'isize', 'usize', 'bool', '&str']


class Variable:
    def __init__(self, var_name, var_type_name):
        self.name = var_name
        self.type_name = var_type_name
        self.type = VariableType.Unset
        self.lifetime_state = LifetimeState.Alive
        # This field is for reference and pointer
        self.reference_to = None
        self.referenced_by = []
        self.children = []
        self.set_type()

    def set_type(self):
        if self.type_name in scalars:
            self.type = VariableType.Scalar
        elif self.type_name.startswith('&'):
            self.type = VariableType.Reference
        elif self.type_name.startswith('*mut'):
            self.type = VariableType.Pointer
        elif self.type_name.startswith('*const'):
            self.type = VariableType.Pointer
        else:
            self.type = VariableType.Object

    def get_type(self):
        return self.type

    def reset_type(self, new_type_name):
        self.type_name = new_type_name
        self.set_type()

    def set_lifetime_state(self, state):
        self.lifetime_state = state

    def reset(self):
        self.lifetime_state = LifetimeState.Alive
        self.reference_to = None
        self.referenced_by = []

    def get_lifetime_state(self):
        return self.lifetime_state

    def set_reference_to(self, var):
        self.reference_to = var

    def set_referenced_by(self, var):
        self.referenced_by.append(var)

    def is_dangling_pointer(self):
        if self.type == VariableType.Pointer and self.reference_to is not None \
                and self.reference_to.get_lifetime_state() == LifetimeState.Terminated:
            return True
        return False

    def add_child_variable(self, child):
        self.children.append(child)

    def find_child_variable_by_name(self, variable_name):
        for child in self.children:
            if child.name == variable_name:
                return child
        return None

    def dump(self):
        print('Name: ' + self.name + ', Type: ' + str(self.type) + ', Type name: ' +
              self.type_name + ', State: ' + str(self.lifetime_state))


