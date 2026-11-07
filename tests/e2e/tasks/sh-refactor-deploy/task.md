`deploy.sh` repeats the same block for each environment. Refactor it so that one function, `deploy_to`, does the work
for a given environment, and the script calls it. `sh deploy.sh ENV` must print exactly the same output as today for
`dev`, `staging` and `prod`, and fail the same way for an unknown environment.
