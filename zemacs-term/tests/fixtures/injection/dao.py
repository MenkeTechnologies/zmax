import re
from sqlalchemy import text


def run(cursor):
    # execute method -> sql
    cursor.execute("SELECT exec_col FROM exec_tbl WHERE id = 1")

    # sqlalchemy text() -> sql
    stmt = text("UPDATE text_tbl SET x = 1")

    # triple-quoted content auto-detect -> sql
    big = """
    SELECT auto_py_a, auto_py_b
    FROM auto_py
    WHERE id = 2
    """

    # regex via re.compile -> regex
    pat = re.compile("[0-9]+regexpy")

    # plain string -> NOT injected
    label = "select an item from the dropdown list"
    return stmt, big, pat, label
