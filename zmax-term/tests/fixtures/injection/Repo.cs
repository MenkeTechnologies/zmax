class Repo {
  void Run(System.Data.IDbConnection conn) {
    // method call -> sql
    var r = conn.ExecuteReader("SELECT cs_col FROM cs_tbl WHERE id = 1");

    // raw-string content auto-detect -> sql
    var q = """
      INSERT INTO cs_auto (a, b) VALUES (1, 2)
      """;

    // plain string -> NOT injected
    var label = "select a report from the csharp dashboard";
  }
}
