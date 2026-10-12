Five monthly exports, jan, feb, mar, apr and may, need converting: `convert-export NAME` writes /app/exports/NAME.csv, and each conversion takes a while.

Then merge the five into /app/merged.csv: the header `id,amount` once, then every row of every export.
