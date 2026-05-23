export interface UserCreateV1 {
  email: string;
  name: string;
}

export interface UserV1 extends UserCreateV1 {
  id: string;
}

export const route = {
  method: "POST",
  path: "/users",
  operationId: "createUser",
  handler: "createUser",
} as const;

export async function createUser(input: UserCreateV1): Promise<UserV1> {
  return {
    id: "user_1",
    email: input.email,
    name: input.name,
  };
}
