fun run(db: android.database.sqlite.SQLiteDatabase) {
    // method call -> sql
    val c = db.rawQuery("SELECT kt_col FROM kt_tbl WHERE id = 1")

    // multiline content auto-detect -> sql
    val q = """
        UPDATE kt_auto SET a = 1 WHERE id = 2
    """

    // plain string -> NOT injected
    val label = "pick a theme from the kotlin settings"
}
