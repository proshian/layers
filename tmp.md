implement a new feature called realtime. analyze history file or somethng    
like this . you need to create an entity called user. each user has its own    
cursor on canvas and its own changes history, one user can see other users     
editing a project, so for this feature i think we need to discuss how to       
implement that better. do we need a single source of truth because it can be   
realtime on the internet, so its logically right to have like a database with  
a project and everyone has a copy and each user action commits to database and 
 broadcasts to everyone. maybe thats right. for the case of one user we just   
keep it simple. when other user joins, we dont care because we have a source   
of truth. the things we dont need to sync is the sample browser and plugins.   
but we need to store recordings and waveforms to database to sync them with    
other clients. we also need to make that if 1 user does some action and calls  
undo it only undos this user action its important. maybe if we have a surreal  
database for storing all project data maybe we can use that to broadcast the   
session to other users. maybe we need a server that handles a storage and sync 
 for lets call it many projects. please discuss this idea with me asking       
questions that comes to your mind  