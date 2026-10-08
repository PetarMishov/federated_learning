Initial idea:

Make a federated learning platform where people/institutions can collaborate.

project structure:
    root -> api -> contains all the backend logic (interactions with db and frontend)
         -> frontend
         -> db -> contains administrator scripts for db management and sql definitions


Current tasks:
    making the initial api, frontend and db
        figuring out the db structure (orgs, permissions etc etc.)
    figuring out how federated learning will actually take place (what if someone is faster, when do updates take place)
        figuring out what steps the users need to take to start this (also how will I get the permission to use the gpu from the frontend?)
        figuring out how we will verify inputs from different users

Database setup, population, and test commands are documented in [db/README.md](db/README.md).

Set up the private Git SSH server before starting the API; see
[git-server/README.md](git-server/README.md). Its SSH identity and Git client are
available through `AppState.git`.
