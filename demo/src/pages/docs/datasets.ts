export const SALES_CSV = `region,status,amount,customer_age,customer_id
North,completed,120,34,C001
South,pending,75,41,C002
North,completed,80,29,C001
West,completed,200,37,C003
South,completed,50,45,C004
West,completed,150,31,C003
East,completed,90,28,C005
`;

export const CUSTOMERS_CSV = `customer_id,segment
C001,Enterprise
C002,SMB
C003,Enterprise
C004,Consumer
C005,SMB
`;

export const ORDERS_RAW_CSV = `order_id,region,channel,gross_amount,discount,status
A100, North ,Web,120,20, Completed
A100, North ,Web,120,20, Completed
A101,South,Store,80,5,pending
A102,West,Web,200,50,completed
A103,East,Partner,90,,Completed
`;

export const DAILY_ORDERS_DAY1_CSV = `order_id,region,amount
A1,North,10
A2,South,20
`;

export const DAILY_ORDERS_DAY2_CSV = `amount,region,order_id
20,South,A2
30,West,A3
`;

export const ORDERS_JSONL = `{"order_id":"J100","region":"North","amount":40,"status":"completed"}
{"order_id":"J101","region":"South","amount":15,"status":"pending"}
{"order_id":"J102","region":"West","amount":60,"status":"completed"}
`;
