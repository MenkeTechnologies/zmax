class UserRepo {
  // @Language annotation -> sql
  @Language("SQL")
  String annotated = "SELECT ann_col FROM ann_tbl";

  // method call -> sql
  void find(java.sql.Statement s) throws Exception {
    var rs = s.executeQuery("SELECT jm_col FROM jm_tbl WHERE id = 1");
  }

  // text block content auto-detect -> sql
  String block = """
    SELECT jb_a, jb_b
    FROM jb_tbl
    WHERE id = 2
    """;

  // plain string -> NOT injected
  String label = "choose an option from the java menu";
}
