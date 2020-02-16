class BasicBlock:
    def __init__(self, name):
        self.name = name
        self.statements = []
        self.marked = 0

    def get_name(self):
        return self.name

    def add_statement(self, statement):
        self.statements.append(statement.strip())

    def get_statements(self):
        return self.statements

    def dump(self):
        print("  BasicBlock: " + self.name)
        for s in self.statements:
            print("    " + s)

    def find_lifetime_termination(self):
        for s in self.statements:
            left = s.split('(')[0]
            if left == 'drop':
                print(self.name + ': ' + s)

    def find_successors(self):
        successors = []
        for statement in self.statements:
            if '->' in statement:
                right = statement.split('->')[1].split('//')[0].strip().strip(';')
                if right.startswith('[') and right.endswith(']'):
                    tokens = right[1:-1].split(', ')
                    for token in tokens:
                        bb_name = token.split(': ')[1]
                        successors.append(bb_name)
                else:
                    if self.is_basic_block_name(right):
                        successors.append(right)
        return successors

    @staticmethod
    def is_basic_block_name(name):
        if name.startswith('bb'):
            return True
        else:
            return False