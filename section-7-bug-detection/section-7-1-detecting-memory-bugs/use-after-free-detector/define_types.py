from enum import Enum


class VariableType(Enum):
    Scalar = 0
    Object = 1
    Reference = 2
    Pointer = 3
    Unset = 999


class LifetimeState(Enum):
    Alive = 0
    Terminated = 1
    Forgot = 2
    Uninitialized = 999


class StatementType(Enum):
    Assignment = 0
    TerminateLifetime = 1
    Other = 10
    Unset = 999


class DestinationType(Enum):
    Local = 0
    Global = 1
    LocalPartial = 2
    GlobalPartial = 3


class AssignmentType(Enum):
    Regular = 0,
    # destination variable cannot be this type
    Reference = 1,
    Dereference = 2,

