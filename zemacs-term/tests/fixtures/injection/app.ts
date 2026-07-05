import { sql } from "./db";

// tagged template -> sql
const a = sql`SELECT tag_col FROM tagged`;

// comment hint -> sql
const b = /* sql */ "UPDATE hint_tbl SET x = 1";

// query method -> sql
db.query("DELETE FROM method_tbl WHERE id = 1");

// content auto-detect -> sql (no tag/hint/method)
const c = "SELECT auto_a, auto_b FROM autos WHERE id = 2";

// styled-components -> css
const Box = styled.div`color: styledblue; padding: 4px;`;

// graphql tag -> graphql
const q = gql`query { graphField }`;

// plain string -> NOT injected
const plain = "please select a plan from the pricing page";
