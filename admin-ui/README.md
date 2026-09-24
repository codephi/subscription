# Subscription Admin UI

Painel interno de consulta do serviço Subscription. Veja o [plano completo](../docs/plano-frontend-administrativo.md) e as [instruções de execução](../README.md#painel-administrativo).

```sh
npm ci
npm run dev
npm test
npm run build
```

A aplicação usa o proxy do Vite para a API em `127.0.0.1:3000`. Defina `VITE_API_BASE_URL` quando precisar de outra origem. Depois de alterar rotas ou DTOs da API, execute `npm run api:types` para atualizar o snapshot OpenAPI e os tipos TypeScript versionados.
