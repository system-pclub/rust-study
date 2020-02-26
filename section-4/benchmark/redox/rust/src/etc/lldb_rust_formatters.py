import lldb
import re
import debugger_pretty_printers_common as rustpp

#===============================================================================
# LLDB Pretty Printing Module for Rust
#===============================================================================

class LldbType(rustpp.Type):

    def __init__(self, ty):
        super(LldbType, self).__init__()
        self.ty = ty
        self.fields = None

    def get_unqualified_type_name(self):
        qualified_name = self.ty.GetName()

        if qualified_name is None:
            return qualified_name

        return rustpp.extract_type_name(qualified_name).replace("&'static ", "&")

    def get_dwarf_type_kind(self):
        type_class = self.ty.GetTypeClass()

        if type_class == lldb.eTypeClassStruct:
            return rustpp.DWARF_TYPE_CODE_STRUCT

        if type_class == lldb.eTypeClassUnion:
            return rustpp.DWARF_TYPE_CODE_UNION

        if type_class == lldb.eTypeClassPointer:
            return rustpp.DWARF_TYPE_CODE_PTR

        if type_class == lldb.eTypeClassArray:
            return rustpp.DWARF_TYPE_CODE_ARRAY

        if type_class == lldb.eTypeClassEnumeration:
            return rustpp.DWARF_TYPE_CODE_ENUM

        return None

    def get_fields(self):
        assert ((self.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_STRUCT) or
                (self.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_UNION))
        if self.fields is None:
            self.fields = list(self.ty.fields)
        return self.fields

    def get_wrapped_value(self):
        return self.ty


class LldbValue(rustpp.Value):
    def __init__(self, lldb_val):
        ty = lldb_val.type
        wty = LldbType(ty)
        super(LldbValue, self).__init__(wty)
        self.lldb_val = lldb_val
        self.children = {}

    def get_child_at_index(self, index):
        child = self.children.get(index)
        if child is None:
            lldb_field = self.lldb_val.GetChildAtIndex(index)
            child = LldbValue(lldb_field)
            self.children[index] = child
        return child

    def as_integer(self):
        return self.lldb_val.GetValueAsUnsigned()

    def get_wrapped_value(self):
        return self.lldb_val


def print_val(lldb_val, internal_dict):
    val = LldbValue(lldb_val)
    type_kind = val.type.get_type_kind()

    if (type_kind == rustpp.TYPE_KIND_REGULAR_STRUCT or
        type_kind == rustpp.TYPE_KIND_REGULAR_UNION or
        type_kind == rustpp.TYPE_KIND_EMPTY):
        return print_struct_val(val,
                                internal_dict,
                                omit_first_field = False,
                                omit_type_name = False,
                                is_tuple_like = False)

    if type_kind == rustpp.TYPE_KIND_STRUCT_VARIANT:
        return print_struct_val(val,
                                internal_dict,
                                omit_first_field = True,
                                omit_type_name = False,
                                is_tuple_like = False)

    if type_kind == rustpp.TYPE_KIND_SLICE:
        return print_vec_slice_val(val, internal_dict)

    if type_kind == rustpp.TYPE_KIND_STR_SLICE:
        return print_str_slice_val(val, internal_dict)

    if type_kind == rustpp.TYPE_KIND_STD_VEC:
        return print_std_vec_val(val, internal_dict)

    if type_kind == rustpp.TYPE_KIND_STD_STRING:
        return print_std_string_val(val, internal_dict)

    if type_kind == rustpp.TYPE_KIND_TUPLE:
        return print_struct_val(val,
                                internal_dict,
                                omit_first_field = False,
                                omit_type_name = True,
                                is_tuple_like = True)

    if type_kind == rustpp.TYPE_KIND_TUPLE_STRUCT:
        return print_struct_val(val,
                                internal_dict,
                                omit_first_field = False,
                                omit_type_name = False,
                                is_tuple_like = True)

    if type_kind == rustpp.TYPE_KIND_CSTYLE_VARIANT:
        return val.type.get_unqualified_type_name()

    if type_kind == rustpp.TYPE_KIND_TUPLE_VARIANT:
        return print_struct_val(val,
                                internal_dict,
                                omit_first_field = True,
                                omit_type_name = False,
                                is_tuple_like = True)

    if type_kind == rustpp.TYPE_KIND_SINGLETON_ENUM:
        return print_val(lldb_val.GetChildAtIndex(0), internal_dict)

    if type_kind == rustpp.TYPE_KIND_PTR:
        return print_pointer_val(val, internal_dict)

    if type_kind == rustpp.TYPE_KIND_FIXED_SIZE_VEC:
        return print_fixed_size_vec_val(val, internal_dict)

    if type_kind == rustpp.TYPE_KIND_REGULAR_ENUM:
        # This is a regular enum, extract the discriminant
        discriminant_val = rustpp.get_discriminant_value_as_integer(val)
        return print_val(lldb_val.GetChildAtIndex(discriminant_val), internal_dict)

    if type_kind == rustpp.TYPE_KIND_COMPRESSED_ENUM:
        encoded_enum_info = rustpp.EncodedEnumInfo(val)
        if encoded_enum_info.is_null_variant():
            return encoded_enum_info.get_null_variant_name()

        non_null_val = encoded_enum_info.get_non_null_variant_val()
        return print_val(non_null_val.get_wrapped_value(), internal_dict)

    # No pretty printer has been found
    return lldb_val.GetValue()


#=--------------------------------------------------------------------------------------------------
# Type-Specialized Printing Functions
#=--------------------------------------------------------------------------------------------------

def print_struct_val(val, internal_dict, omit_first_field, omit_type_name, is_tuple_like):
    """
    Prints a struct, tuple, or tuple struct value with Rust syntax.
    Ignores any fields before field_start_index.
    """
    assert (val.type.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_STRUCT or
            val.type.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_UNION)

    if omit_type_name:
        type_name = ""
    else:
        type_name = val.type.get_unqualified_type_name()

    if is_tuple_like:
        template = "%(type_name)s(%(body)s)"
        separator = ", "
    else:
        template = "%(type_name)s {\n%(body)s\n}"
        separator = ", \n"

    fields = val.type.get_fields()

    def render_child(child_index):
        this = ""
        if not is_tuple_like:
            field_name = fields[child_index].name
            this += field_name + ": "

        field_val = val.get_child_at_index(child_index)

        if not field_val.get_wrapped_value().IsValid():
            field = fields[child_index]
            # LLDB is not good at handling zero-sized values, so we have to help
            # it a little
            if field.GetType().GetByteSize() == 0:
                return this + rustpp.extract_type_name(field.GetType().GetName())
            else:
                return this + "<invalid value>"

        return this + print_val(field_val.get_wrapped_value(), internal_dict)

    if omit_first_field:
        field_start_index = 1
    else:
        field_start_index = 0

    body = separator.join([render_child(idx) for idx in range(field_start_index, len(fields))])

    return template % {"type_name": type_name,
                       "body": body}

def print_pointer_val(val, internal_dict):
    """Prints a pointer value with Rust syntax"""
    assert val.type.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_PTR
    sigil = "&"
    type_name = val.type.get_unqualified_type_name()
    if type_name and type_name[0:1] in ["&", "*"]:
        sigil = type_name[0:1]

    return sigil + hex(val.as_integer())


def print_fixed_size_vec_val(val, internal_dict):
    assert val.type.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_ARRAY
    lldb_val = val.get_wrapped_value()

    output = "["

    for i in range(lldb_val.num_children):
        output += print_val(lldb_val.GetChildAtIndex(i), internal_dict)
        if i != lldb_val.num_children - 1:
            output += ", "

    output += "]"
    return output


def print_vec_slice_val(val, internal_dict):
    (length, data_ptr) = rustpp.extract_length_and_ptr_from_slice(val)
    return "&[%s]" % print_array_of_values(val.get_wrapped_value().GetName(),
                                           data_ptr,
                                           length,
                                           internal_dict)


def print_std_vec_val(val, internal_dict):
    (length, data_ptr, cap) = rustpp.extract_length_ptr_and_cap_from_std_vec(val)
    return "vec![%s]" % print_array_of_values(val.get_wrapped_value().GetName(),
                                              data_ptr,
                                              length,
                                              internal_dict)

def print_str_slice_val(val, internal_dict):
    (length, data_ptr) = rustpp.extract_length_and_ptr_from_slice(val)
    return read_utf8_string(data_ptr, length)

def print_std_string_val(val, internal_dict):
    vec = val.get_child_at_index(0)
    (length, data_ptr, cap) = rustpp.extract_length_ptr_and_cap_from_std_vec(vec)
    return read_utf8_string(data_ptr, length)

#=--------------------------------------------------------------------------------------------------
# Helper Functions
#=--------------------------------------------------------------------------------------------------

def print_array_of_values(array_name, data_ptr_val, length, internal_dict):
    """Prints a contiguous memory range, interpreting it as values of the
       pointee-type of data_ptr_val."""

    data_ptr_type = data_ptr_val.type
    assert data_ptr_type.get_dwarf_type_kind() == rustpp.DWARF_TYPE_CODE_PTR

    element_type = data_ptr_type.get_wrapped_value().GetPointeeType()
    element_type_size = element_type.GetByteSize()

    start_address = data_ptr_val.as_integer()
    raw_value = data_ptr_val.get_wrapped_value()

    def render_element(i):
        address = start_address + i * element_type_size
        element_val = raw_value.CreateValueFromAddress(array_name + ("[%s]" % i),
                                                       address,
                                                       element_type)
        return print_val(element_val, internal_dict)

    return ', '.join([render_element(i) for i in range(length)])


def read_utf8_string(ptr_val, byte_count):
    if byte_count == 0:
        return '""'
    error = lldb.SBError()
    process = ptr_val.get_wrapped_value().GetProcess()
    data = process.ReadMemory(ptr_val.as_integer(), byte_count, error)
    if error.Success():
        return '"%s"' % data.decode(encoding='UTF-8')
    else:
        return '<error: %s>' % error.GetCString()
