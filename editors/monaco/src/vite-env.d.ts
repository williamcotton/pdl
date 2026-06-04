declare module "*.css";

declare module "*?url" {
  const url: string;
  export default url;
}

declare module "*?worker" {
  const WorkerFactory: {
    new (): Worker;
  };
  export default WorkerFactory;
}
