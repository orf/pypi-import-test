# PyPi Code

This repository contains all the code released on PyPi between {first_release} and {last_release}.

{total_projects} projects uploaded {total_releases} releases. 

Top 50 packages by number of releases ([JSON full index](./index.json)):

| Package   | Count |
|-----------|-------|
{{ for value in table -}}
| [{value.0}]({repo_url}/tree/import/{value.0}) | {value.1} |
{{ endfor }}