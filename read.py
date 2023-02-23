import json
import sys
import pprint


def parse_ast(item: dict):
    print(item)


for line in sys.stdin.readlines():
    line = json.loads(line)
    for value in line:
        parse_ast(value)
