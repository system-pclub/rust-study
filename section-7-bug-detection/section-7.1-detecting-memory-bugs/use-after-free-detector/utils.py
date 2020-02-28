import re
from define_types import *
import logging


def is_local_variable(variable_name):
    """
    @ variable_name: should be single world and remove all '(', ')', '*', '&'
    @ return value: -1 if this is not a local variable
                    variable_num if this is a local variable
    """
    pattern = r'_\d+'
    m = re.search(pattern, variable_name)
    return bool(m)


def find_local_variable_name(src_str):
    pattern = r'_\d+'
    m = re.search(pattern, src_str)

    assert(bool(m))
    return str(m.group(0))


def find_global_variable_name_and_type(search_str):
    pattern = r'(.+): (.+)'
    m = re.search(pattern, search_str)

    assert(bool(m))
    variable_name = str(m.group(1))
    variable_type = str(m.group(2))
    return variable_name, variable_type


def find_variable_name_and_type(search_str):
    """
    move must be handled before calling this function
    @return value assignment_type, variable_vector
    """
    assignment_type = AssignmentType.Regular
    variable_vector = []

    # This must be put before '&'
    if search_str.startswith('&mut '):
        assignment_type = AssignmentType.Reference
        search_str = search_str.strip('&mut ')

    if search_str.startswith('&'):
        assignment_type = AssignmentType.Reference
        search_str = search_str.strip('&')

    pattern = r'(.+)\.(\d+): (.+)'
    m = re.search(pattern, search_str)

    if m:
        parent_variable_str = str(m.group(1)).strip('(').strip(')')
        child_variable_name = str(m.group(2)).strip('(').strip(')')
        child_variable_type = str(m.group(3)).strip('(').strip(')')

        if parent_variable_str.startswith('*'):
            if assignment_type == AssignmentType.Regular:
                assignment_type = AssignmentType.Dereference
            else:
                logging.warning("Handling &(*a) may have some issue: %s", search_str)

        if is_local_variable(parent_variable_str):
            parent_variable_name = find_local_variable_name(parent_variable_str)
            variable_vector.append((parent_variable_name, None))
        else:
            parent_variable_name, parent_variable_type = find_global_variable_name_and_type(parent_variable_str)
            variable_vector.append((parent_variable_name, parent_variable_type))

        variable_vector.append((child_variable_name, child_variable_type))
    else:
        variable_str = search_str.strip('(').strip(')')

        if variable_str.startswith('*'):
            if assignment_type == AssignmentType.Regular:
                assignment_type = AssignmentType.Dereference
            else:
                logging.warning("Handling &(*a) may have some issue: %s", search_str)

        if is_local_variable(variable_str):
            variable_name = find_local_variable_name(variable_str)
            variable_vector.append((variable_name, None))
        else:
            variable_name, variable_type = find_global_variable_name_and_type(variable_str)
            variable_vector.append((variable_name, variable_type))

    logging.debug('Find variable, assignment type: %s, variable_vector: %s',
                  str(assignment_type), str(variable_vector))

    return assignment_type, variable_vector


def set_reference(_from, _to):
    """
    Set _from is reference to _to
    Set _to is referenced by _from if _to is not None
    """
    _from.set_reference_to(_to)
    if _to:
        _to.set_referenced_by(_from)




